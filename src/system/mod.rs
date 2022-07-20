mod cell;
mod traits;

use std::marker::PhantomData;

use crate::{util::TupleCombine, ArchetypeId, ComponentId};

pub use cell::*;
pub use traits::*;

pub struct SystemBuilder<T> {
    data: T,
    name: Option<String>,
}

impl SystemBuilder<()> {
    /// Creates a new empty system builder.
    pub fn new() -> Self {
        Self {
            data: (),
            name: None,
        }
    }
}

impl<Args> SystemBuilder<Args> {
    /// Add a new query to the system
    pub fn with<S>(self, other: S) -> SystemBuilder<Args::PushRight>
    where
        S: SystemAccess + for<'x> SystemData<'x>,
        Args: TupleCombine<S>,
    {
        SystemBuilder {
            name: self.name,
            data: self.data.push_right(other),
        }
    }

    /// Set the systems name
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn build<F, Ret>(self, func: F) -> System<F, Args, Ret>
    where
        Args: for<'a> SystemData<'a> + 'static,
        F: for<'a> SystemFn<
            'a,
            (&'a SystemContext<'a>, &'a mut Args),
            <Args as SystemData<'a>>::Data,
            Ret,
        >,
    {
        System::new(self.name, func, self.data)
    }
}

/// Holds the data and an inner system satisfying all dependencies
pub struct System<F, Args, Ret> {
    name: Option<String>,
    data: Args,
    func: F,
    _marker: PhantomData<Ret>,
}

impl<F, Args, Ret> System<F, Args, Ret>
where
    for<'a> Args: SystemData<'a> + 'a,
    F: for<'a> SystemFn<
        'a,
        (&'a SystemContext<'a>, &'a mut Args),
        <Args as SystemData<'a>>::Data,
        Ret,
    >,
{
    pub fn new(name: Option<String>, func: F, data: Args) -> Self {
        Self {
            name,
            data,
            func,
            _marker: PhantomData,
        }
    }

    /// Convert to a type erased Send + Sync system
    pub fn boxed(self) -> BoxedSystem
    where
        Args: Send + Sync + 'static,
        Ret: Send + Sync + 'static,
        F: Send + Sync + 'static,
        Self: for<'a> SystemFn<'a, &'a SystemContext<'a>, (), eyre::Result<()>>,
    {
        BoxedSystem::new(self)
    }
}
impl System<(), (), ()> {
    pub fn builder() -> SystemBuilder<()> {
        SystemBuilder::new()
    }
}

impl<'a, F, Args> SystemFn<'a, &'a SystemContext<'a>, (), eyre::Result<()>> for System<F, Args, ()>
where
    Args: SystemData<'a> + 'a,
    F: SystemFn<'a, (&'a SystemContext<'a>, &'a mut Args), Args::Data, ()>,
{
    fn execute(&'a mut self, ctx: &'a SystemContext<'a>) -> eyre::Result<()> {
        self.func.execute((ctx, &mut self.data));
        Ok(())
    }

    fn describe(&self, f: &mut dyn std::fmt::Write) {
        if let Some(ref name) = self.name {
            write!(f, "{name}: ").unwrap();
        }
        self.func.describe(f);
    }

    fn access(&'a mut self, ctx: &'a SystemContext<'a>) -> Vec<Access> {
        self.func.access((ctx, &mut self.data))
    }
}

impl<'a, F, Args, E> SystemFn<'a, &'a SystemContext<'a>, (), eyre::Result<()>>
    for System<F, Args, std::result::Result<(), E>>
where
    Args: SystemData<'a> + 'a,
    F: SystemFn<'a, (&'a SystemContext<'a>, &'a mut Args), Args::Data, std::result::Result<(), E>>,
    E: Into<eyre::Report> + 'static,
{
    fn execute(&'a mut self, ctx: &'a SystemContext<'a>) -> eyre::Result<()> {
        self.func.execute((ctx, &mut self.data)).map_err(|v| {
            v.into().wrap_err(format!(
                "Failed to execute system: {}",
                self.name.as_deref().unwrap_or_default()
            ))
        })
    }

    fn describe(&self, f: &mut dyn std::fmt::Write) {
        if let Some(ref name) = self.name {
            write!(f, "{name}: ").unwrap();
        }
        self.func.describe(f);
    }

    fn access(&'a mut self, ctx: &'a SystemContext<'a>) -> Vec<Access> {
        self.func.access((ctx, &mut self.data))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccessKind {
    /// Borrow a single component of an archetype
    Archetype {
        id: ArchetypeId,
        component: ComponentId,
    },
    /// Borrow the whole world
    World,
    CommandBuffer,
}

impl AccessKind {
    /// Returns `true` if the access kind is [`Archetype`].
    ///
    /// [`Archetype`]: AccessKind::Archetype
    #[must_use]
    pub fn is_archetype(&self) -> bool {
        matches!(self, Self::Archetype { .. })
    }

    /// Returns `true` if the access kind is [`World`].
    ///
    /// [`World`]: AccessKind::World
    #[must_use]
    pub fn is_world(&self) -> bool {
        matches!(self, Self::World)
    }

    /// Returns `true` if the access kind is [`CommandBuffer`].
    ///
    /// [`CommandBuffer`]: AccessKind::CommandBuffer
    #[must_use]
    pub fn is_command_buffer(&self) -> bool {
        matches!(self, Self::CommandBuffer)
    }
}
#[derive(Debug, Clone)]
pub struct Access {
    pub kind: AccessKind,
    pub mutable: bool,
}

impl Access {
    /// Returns true it both accesses can coexist
    pub fn is_compatible_with(&self, other: &Self) -> bool {
        // Any access to the whole world breaks concurrency, sadly
        if (!self.kind.is_command_buffer() && other.kind.is_world())
            || (!other.kind.is_command_buffer() && self.kind.is_world())
        {
            false
        } else if self.kind != other.kind {
            true
        } else {
            self.mutable == false && other.mutable == false
        }
    }
}

/// A type erased system
pub struct BoxedSystem {
    inner: Box<dyn for<'x> SystemFn<'x, &'x SystemContext<'x>, (), eyre::Result<()>> + Send + Sync>,
}

impl BoxedSystem {
    pub fn new(
        system: impl for<'x> SystemFn<'x, &'x SystemContext<'x>, (), eyre::Result<()>>
            + Send
            + Sync
            + 'static,
    ) -> Self {
        Self {
            inner: Box::new(system),
        }
    }

    pub fn execute<'a>(&'a mut self, ctx: &'a SystemContext<'a>) -> eyre::Result<()> {
        self.inner.execute(ctx)
    }

    pub fn describe<'a>(&self, f: &mut dyn std::fmt::Write) {
        self.inner.describe(f)
    }

    pub fn access<'a>(&'a mut self, ctx: &'a SystemContext<'a>) -> Vec<Access> {
        self.inner.access(ctx)
    }
}

impl<T> From<T> for BoxedSystem
where
    T: for<'x> SystemFn<'x, &'x SystemContext<'x>, (), eyre::Result<()>> + Send + Sync + 'static,
{
    fn from(system: T) -> Self {
        Self::new(system)
    }
}

#[cfg(test)]
mod test {

    use crate::{component, All, CommandBuffer, Component, EntityBuilder, Query, QueryData, World};

    use super::*;

    #[test]
    fn system_builder() {
        component! {
            a: String,
            b: i32,
        };

        let mut world = World::new();

        let id = EntityBuilder::new()
            .set(a(), "Foo".to_string())
            .set(b(), 5)
            .spawn(&mut world);

        let mut system = System::builder()
            .with(Query::new(a()))
            // .with(Query::new(b()))
            .build(|mut a: QueryData<Component<String>>| assert_eq!(a.prepare().iter().count(), 1));

        let mut fallible = System::builder()
            .with_name("Fallible")
            .with(Query::new(b()))
            .build(
                move |mut query: QueryData<Component<i32>>| -> eyre::Result<()> {
                    // Lock archetypes
                    let mut query = query.prepare();
                    let item: &i32 = query.get(id)?;
                    eprintln!("Item: {item}");

                    Ok(())
                },
            );

        let mut cmd = CommandBuffer::new();

        let ctx = SystemContext::new(&mut world, &mut cmd);
        system.execute(&ctx).unwrap();

        fallible.execute(&ctx).unwrap();

        world.remove(id, b()).unwrap();

        let mut boxed = fallible.boxed();

        let ctx = SystemContext::new(&mut world, &mut cmd);
        let res = boxed.execute(&ctx);
        eprintln!("{:?}", res.as_ref().unwrap_err());
        assert!(res.is_err());
    }
}
