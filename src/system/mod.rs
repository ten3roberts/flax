mod context;
mod traits;

use core::fmt;
use std::{
    any::{type_name, TypeId},
    fmt::Formatter,
    marker::PhantomData,
};

use crate::{
    fetch::PreparedFetch, util::TupleCombine, ArchetypeId, CommandBuffer, ComponentId, Fetch,
    FetchItem, Filter, Query, QueryData, World,
};

pub use context::*;
use eyre::Context;
pub use traits::*;

/// A system builder which allows incrementally adding data to a system
/// function.
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

impl<'a, Func, Q, F> SystemFn<'a, QueryData<'a, Q, F>, ()> for ForEach<Func>
where
    for<'x> Q: Fetch<'x> + std::fmt::Debug,
    for<'x> F: Filter<'x> + std::fmt::Debug,
    for<'x> Func: FnMut(<Q as FetchItem<'x>>::Item),
{
    fn execute(&mut self, mut data: QueryData<Q, F>) {
        for item in &mut data.borrow() {
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

    fn access(&self, _: &World) -> Vec<Access> {
        vec![]
    }

    fn name(&self) -> String {
        format!(
            "for_each<{}, filter: {}>",
            tynm::type_name::<<<Q as Fetch<'static>>::Prepared as PreparedFetch>::Item>(),
            tynm::type_name::<F>()
        )
    }
}

impl<Q, F> SystemBuilder<(Query<Q, F>,)>
where
    for<'x> Q: Fetch<'x> + std::fmt::Debug + 'static,
    for<'x> F: Filter<'x> + std::fmt::Debug + 'static,
{
    /// Execute a function for each item in the query
    pub fn for_each<Func>(self, func: Func) -> System<ForEach<Func>, Query<Q, F>, ()>
    where
        for<'x> Func: FnMut(<Q as FetchItem<'x>>::Item),
    {
        System::new(
            self.name.unwrap_or_else(|| type_name::<Func>().to_string()),
            ForEach { func },
            self.data.0,
        )
    }
}

impl<Args> SystemBuilder<Args> {
    /// Add a new query to the system
    pub fn with<S>(self, other: S) -> SystemBuilder<Args::PushRight>
    where
        S: for<'x> SystemData<'x>,
        Args: TupleCombine<S>,
    {
        SystemBuilder {
            name: self.name,
            data: self.data.push_right(other),
        }
    }

    /// Access data data mutably in the system
    pub fn write<T>(self) -> SystemBuilder<Args::PushRight>
    where
        Args: TupleCombine<Write<T>>,
        Write<T>: for<'x> SystemData<'x>,
    {
        self.with(Write::<T>::default())
    }

    /// Access data data mutably in the system
    pub fn read<T>(self) -> SystemBuilder<Args::PushRight>
    where
        Args: TupleCombine<Read<T>>,
        Read<T>: for<'x> SystemData<'x>,
    {
        self.with(Read::<T>::default())
    }

    /// Set the systems name
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Access a shared resource mutable in the system.
    ///
    /// This is useful to avoid sharing `Arc<Mutex<_>>` and locking for each
    /// system. In addition, the resource will be taken into account for the
    /// schedule paralellization and will as such not block.
    pub fn with_resource<T>(self, resource: SharedResource<T>) -> SystemBuilder<Args::PushRight>
    where
        Args: TupleCombine<SharedResource<T>>,
        T: Send + 'static,
    {
        self.with(resource)
    }

    /// Creates the system by suppling a function to act upon the systems data,
    /// like queries and world accesses.
    pub fn build<Func, Ret>(self, func: Func) -> System<Func, Args, Ret>
    where
        Args: for<'a> SystemData<'a> + 'static,
        Func: for<'this, 'a> SystemFn<'this, <Args as SystemData<'a>>::Value, Ret>,
    {
        System::new(
            self.name.unwrap_or_else(|| type_name::<Func>().to_string()),
            func,
            self.data,
        )
    }
}

/// Holds the data and an inner system satisfying all dependencies
pub struct System<F, Args, Ret> {
    name: String,
    data: Args,
    func: F,
    _marker: PhantomData<Ret>,
}

struct FormatWith<F> {
    func: F,
}

impl<F> fmt::Debug for FormatWith<F>
where
    F: Fn(&mut Formatter<'_>) -> fmt::Result,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        (self.func)(f)
    }
}

impl<'this, F, Args, Err> SystemFn<'this, &'this SystemContext<'this>, eyre::Result<()>>
    for System<F, Args, Result<(), Err>>
where
    Args: for<'x> SystemData<'x>,
    F: for<'x> SystemFn<'x, <Args as SystemData<'x>>::Value, Result<(), Err>>,
    Err: Into<eyre::Error>,
{
    fn execute(&'this mut self, ctx: &'this SystemContext<'this>) -> eyre::Result<()> {
        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!("execute", name = self.name).entered();

        let data = self
            .data
            .acquire(ctx)
            .wrap_err_with(|| format!("Failed to bind system data for {}", self.name))?;

        let res: eyre::Result<()> = self.func.execute(data).map_err(Into::into);
        if let Err(err) = res {
            return Err(err.wrap_err(format!("Failed to execute system: {:?}", self)));
        }

        Ok(())
    }

    fn describe(&self, f: &mut fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: ", self.name)?;

        self.func.describe(f)
    }

    fn access(&self, world: &World) -> Vec<Access> {
        self.data.access(world)
    }

    fn name(&self) -> String {
        self.name.clone()
    }
}

impl<'this, F, Args> SystemFn<'this, &'this SystemContext<'this>, eyre::Result<()>>
    for System<F, Args, ()>
where
    Args: SystemData<'this>,
    F: SystemFn<'this, Args::Value, ()>,
{
    fn execute(&'this mut self, ctx: &'this SystemContext<'this>) -> eyre::Result<()> {
        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!("execute", name = self.name).entered();

        let data = self
            .data
            .acquire(ctx)
            .wrap_err_with(|| format!("Failed to bind system data for {}", self.name))?;

        self.func.execute(data);

        Ok(())
    }

    fn describe(&self, f: &mut fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: ", self.name)?;

        self.func.describe(f)
    }

    fn access(&self, world: &World) -> Vec<Access> {
        self.data.access(world)
    }

    fn name(&self) -> String {
        self.name.clone()
    }
}

impl<F, Args, Ret> fmt::Debug for System<F, Args, Ret>
where
    Self: for<'x> SystemFn<'x, &'x SystemContext<'x>, eyre::Result<()>>,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        self.describe(f)
    }
}

impl<F, Args, Ret> System<F, Args, Ret> {
    pub(crate) fn new(name: String, func: F, data: Args) -> Self {
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
        Ret: Send + Sync + 'static,
        Args: Send + Sync + 'static,
        F: Send + Sync + 'static,
        Self: for<'x> SystemFn<'x, &'x SystemContext<'x>, eyre::Result<()>>,
    {
        BoxedSystem::new(self)
    }
}

impl System<(), (), ()> {
    /// See [crate::SystemBuilder]
    pub fn builder() -> SystemBuilder<()> {
        SystemBuilder::new()
    }
}

impl<F, Args, Ret> System<F, Args, Ret> {
    /// Run the system on the world. Any commands will be applied
    pub fn run_on<'a>(&'a mut self, world: &'a mut World) -> Ret
    where
        Ret: 'static,
        for<'x> Args: SystemData<'x>,
        for<'x> F: SystemFn<'x, <Args as SystemData<'x>>::Value, Ret>,
    {
        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!("run_on", name = self.name).entered();

        let mut cmd = CommandBuffer::new();
        let ctx = SystemContext::new(world, &mut cmd);

        let data = self.data.acquire(&ctx).expect("Failed to bind data");

        let ret = self.func.execute(data);
        cmd.apply(world).expect("Failed to apply commandbuffer");
        ret
    }
}

#[derive(Hash, Debug, Clone, PartialEq, Eq)]
/// Describes a kind of access
pub enum AccessKind {
    /// Borrow a single component of an archetype
    Archetype {
        /// The archetype id
        id: ArchetypeId,
        /// The accessed component
        component: ComponentId,
    },
    /// Borrow a single change event of an archetype
    ChangeEvent {
        /// The archetype id
        id: ArchetypeId,
        /// The accessed component
        component: ComponentId,
    },
    /// A unit struct works as a synchronization barrier
    External(TypeId),
    /// Borrow the whole world
    World,
    /// Borrow the commandbuffer
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
/// Describes an access for a system, allowing for dependency resolution and
/// multithreading
pub struct Access {
    /// The kind of access
    pub kind: AccessKind,
    /// shared or unique/mutable access
    pub mutable: bool,
}

impl Access {
    /// Returns true it both accesses can coexist
    pub fn is_compatible_with(&self, other: &Self) -> bool {
        !(self.kind == other.kind && self.mutable || other.mutable)
    }
}

/// A system which should never be run.
/// Is essentially a `None` variant system.
pub(crate) struct NeverSystem;

impl<'a> SystemFn<'a, &'a SystemContext<'a>, eyre::Result<()>> for NeverSystem {
    fn execute(&'a mut self, _: &'a SystemContext<'a>) -> eyre::Result<()> {
        panic!("This system should never be executed as it is a placeholder");
    }

    fn describe(&self, f: &mut std::fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "NeverSystem")
    }

    fn access(&self, _: &World) -> Vec<Access> {
        vec![]
    }

    fn name(&self) -> String {
        "NeverSystem".to_string()
    }
}

/// A type erased system
pub struct BoxedSystem {
    inner: Box<dyn for<'x> SystemFn<'x, &'x SystemContext<'x>, eyre::Result<()>> + Send + Sync>,
}

impl std::fmt::Debug for BoxedSystem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.inner.describe(f)
    }
}

impl BoxedSystem {
    /// Creates a new boxed system from any other kind of system
    pub fn new(
        system: impl for<'x> SystemFn<'x, &'x SystemContext<'x>, eyre::Result<()>>
            + Send
            + Sync
            + 'static,
    ) -> Self {
        Self {
            inner: Box::new(system),
        }
    }

    /// Execute the system with the provided context
    pub fn execute<'a>(&'a mut self, ctx: &'a SystemContext<'a>) -> eyre::Result<()> {
        self.inner.execute(ctx)
    }

    /// Same as execute but creates and applied a temporary commandbuffer
    pub fn run_on<'a>(&'a mut self, world: &'a mut World) -> eyre::Result<()> {
        let mut cmd = CommandBuffer::new();
        let ctx = SystemContext::new(world, &mut cmd);
        self.inner.execute(&ctx)?;
        Ok(())
    }

    /// Describes the system held within
    pub fn describe(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.inner.describe(f)
    }

    /// Returns the accesses of the system held within
    pub fn access(&self, world: &World) -> Vec<Access> {
        self.inner.access(world)
    }

    /// Returns the boxed system's name
    pub fn name(&self) -> String {
        self.inner.name()
    }
}

impl<T> From<T> for BoxedSystem
where
    T: for<'x> SystemFn<'x, &'x SystemContext<'x>, eyre::Result<()>> + Send + Sync + 'static,
{
    fn from(system: T) -> Self {
        Self::new(system)
    }
}

#[cfg(test)]
mod test {

    use crate::{component, CommandBuffer, Component, EntityBuilder, Query, QueryBorrow, World};

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
            .build(|mut a: QueryBorrow<Component<String>>| assert_eq!(a.iter().count(), 1));

        let mut fallible = System::builder()
            // .with_name("Fallible")
            .with(Query::new(b()))
            .build(
                move |mut query: QueryBorrow<Component<i32>>| -> eyre::Result<()> {
                    // Lock archetypes
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
