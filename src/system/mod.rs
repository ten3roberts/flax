mod cell;
mod traits;

use core::fmt;
use std::{any::type_name, marker::PhantomData};

use crate::{
    util::TupleCombine, ArchetypeId, CommandBuffer, ComponentId, Fetch, Filter, PreparedFetch,
    Query, QueryData, World,
};

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

impl Default for SystemBuilder<()> {
    fn default() -> Self {
        Self::new()
    }
}

#[doc(hidden)]
pub struct ForEach<F> {
    func: F,
}

impl<'a, Func, Q, F>
    SystemFn<'a, (&'a SystemContext<'a>, &'a mut Query<Q, F>), QueryData<'a, Q, F>, ()>
    for ForEach<Func>
where
    for<'x> Q: Fetch<'x> + std::fmt::Debug,
    for<'x, 'y> F: Filter<'x, 'y> + std::fmt::Debug,
    for<'x> Func: FnMut(<<Q as Fetch<'x>>::Prepared as PreparedFetch>::Item),
{
    fn execute(&mut self, (ctx, data): (&'a SystemContext<'a>, &'a mut Query<Q, F>)) {
        let mut data = data.get(ctx).expect("Failed to get system data");
        for item in &mut data.prepare() {
            (self.func)(item)
        }
    }

    fn describe(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "for_each<{}, filter: {}>",
            tynm::type_name::<<<Q as Fetch<'static>>::Prepared as PreparedFetch>::Item>(),
            tynm::type_name::<F>()
        )
    }

    fn access(
        &'a mut self,
        (ctx, data): (&'a SystemContext<'a>, &'a mut Query<Q, F>),
    ) -> Vec<Access> {
        data.access(&ctx.world().unwrap())
    }
}

impl<Q, F> SystemBuilder<(Query<Q, F>,)>
where
    for<'x> Q: Fetch<'x> + std::fmt::Debug + 'static,
    for<'x, 'y> F: Filter<'x, 'y> + std::fmt::Debug + 'static,
{
    pub fn for_each<Func>(self, func: Func) -> System<ForEach<Func>, Query<Q, F>, ()>
    where
        for<'x> Func: FnMut(<<Q as Fetch<'x>>::Prepared as PreparedFetch>::Item),
    {
        System::new(self.name, ForEach { func }, self.data.0)
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

    /// Access the world mutably in the query.
    /// This adds a `Write<World>` argument
    pub fn with_world(self) -> SystemBuilder<Args::PushRight>
    where
        Args: TupleCombine<Writable<World>>,
    {
        self.with(Writable::<World>::default())
    }

    /// Access the commandbuffer mutably in the query.
    /// This adds a `Write<CommandBuffer>` argument
    pub fn with_cmd(self) -> SystemBuilder<Args::PushRight>
    where
        Args: TupleCombine<Writable<CommandBuffer>>,
    {
        self.with(Writable::<CommandBuffer>::default())
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
    #[tracing::instrument(skip_all, fields(name = self.name.as_deref().unwrap_or_default()))]
    fn execute(&'a mut self, ctx: &'a SystemContext<'a>) -> eyre::Result<()> {
        self.func.execute((ctx, &mut self.data));
        Ok(())
    }

    fn describe(&self, f: &mut std::fmt::Formatter<'_>) -> fmt::Result {
        if let Some(ref name) = self.name {
            write!(f, "{name}: ")?;
        }

        self.func.describe(f)
    }

    fn access(&'a mut self, ctx: &'a SystemContext<'a>) -> Vec<Access> {
        self.func.access((ctx, &mut self.data))
    }
}

impl<F, Args, Ret> System<F, Args, Ret> {
    /// Run the system on the world. Any commands will be applied
    #[tracing::instrument(skip_all, fields(name = ?self.name))]
    pub fn run_on(&mut self, world: &mut World) -> Ret
    where
        for<'x> Args: SystemData<'x> + 'x,
        for<'a> F: SystemFn<
            'a,
            (&'a SystemContext<'a>, &'a mut Args),
            <Args as SystemData<'a>>::Data,
            Ret,
        >,
    {
        let mut cmd = CommandBuffer::new();
        let ctx = SystemContext::new(world, &mut cmd);
        let ret = self.func.execute((&ctx, &mut self.data));
        cmd.apply(world).expect("Failed to apply commandbuffer");
        ret
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

    fn describe(&self, f: &mut std::fmt::Formatter<'_>) -> fmt::Result {
        if let Some(ref name) = self.name {
            write!(f, "{name}: ")?;
        }

        self.func.describe(f)
    }

    fn access(&'a mut self, ctx: &'a SystemContext<'a>) -> Vec<Access> {
        self.func.access((ctx, &mut self.data))
    }
}

#[derive(Hash, Debug, Clone, PartialEq, Eq)]
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

#[derive(Hash, Debug, Clone, PartialEq, Eq)]
pub struct Access {
    pub kind: AccessKind,
    pub mutable: bool,
}

impl Access {
    /// Returns true it both accesses can coexist
    pub fn is_compatible_with(&self, other: &Self) -> bool {
        self.kind != other.kind || !(self.mutable || other.mutable)
    }
}

/// A system which should never be run.
/// Is essentially a `None` variant system.
pub(crate) struct NeverSystem;

impl<'a> SystemFn<'a, &'a SystemContext<'a>, (), eyre::Result<()>> for NeverSystem {
    fn execute(&'a mut self, _: &'a SystemContext<'a>) -> eyre::Result<()> {
        panic!("This system should never be executed as it is a placeholde");
    }

    fn describe(&self, f: &mut std::fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "NeverSystem")
    }

    fn access(&'a mut self, _: &'a SystemContext<'a>) -> Vec<Access> {
        vec![]
    }
}

/// A type erased system
pub struct BoxedSystem {
    inner: Box<dyn for<'x> SystemFn<'x, &'x SystemContext<'x>, (), eyre::Result<()>> + Send + Sync>,
}

impl std::fmt::Debug for BoxedSystem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.inner.describe(f);
        Ok(())
    }
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

    pub fn run_on<'a>(&'a mut self, world: &'a mut World) -> eyre::Result<()> {
        let mut cmd = CommandBuffer::new();
        let ctx = SystemContext::new(world, &mut cmd);
        self.inner.execute(&ctx)?;
        cmd.apply(world)?;
        Ok(())
    }

    pub fn describe(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
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

    use crate::{component, CommandBuffer, Component, EntityBuilder, Query, QueryData, World};

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
        let _ = res.unwrap_err();
    }
}
