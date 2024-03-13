mod context;
mod input;
mod traits;

use crate::{
    archetype::{ArchetypeId, ArchetypeInfo},
    component::ComponentKey,
    query::{QueryData, QueryStrategy},
    util::TuplePush,
    CommandBuffer, Fetch, FetchItem, Query, World,
};
use alloc::{
    boxed::Box,
    collections::BTreeMap,
    format,
    string::{String, ToString},
    vec::Vec,
};
use core::{
    any::{type_name, TypeId},
    fmt::{self, Formatter},
    marker::PhantomData,
};

pub use context::*;
pub use input::IntoInput;
pub use traits::{AsBorrowed, SystemAccess, SystemData, SystemFn};

use self::traits::{WithCmd, WithCmdMut, WithInput, WithInputMut, WithWorld, WithWorldMut};

#[cfg(feature = "rayon")]
use rayon::prelude::{ParallelBridge, ParallelIterator};

/// A system builder which allows incrementally adding data to a system
/// function.
pub struct SystemBuilder<Args> {
    args: Args,
    name: Option<String>,
}

impl SystemBuilder<()> {
    /// Creates a new empty system builder.
    pub fn new() -> Self {
        Self {
            args: (),
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
pub struct ForEach<Func> {
    func: Func,
}

impl<'a, Func, Q, F> SystemFn<'a, (QueryData<'a, Q, F>,), ()> for ForEach<Func>
where
    for<'x> Q: Fetch<'x>,
    for<'x> F: Fetch<'x>,
    for<'x> Func: FnMut(<Q as FetchItem<'x>>::Item),
{
    fn execute(&mut self, mut data: (QueryData<Q, F>,)) {
        for item in &mut data.0.borrow() {
            (self.func)(item)
        }
    }
}

/// Execute a function for each item in the query in parallel batches
#[cfg(feature = "rayon")]
pub struct ParForEach<F> {
    func: F,
}

#[cfg(feature = "rayon")]
impl<'a, Func, Q, F> SystemFn<'a, (QueryData<'a, Q, F>,), ()> for ParForEach<Func>
where
    for<'x> Q: Fetch<'x>,
    for<'x> F: Fetch<'x>,
    for<'x> <crate::filter::Filtered<Q, F> as Fetch<'x>>::Prepared: Send,
    for<'x, 'y> <<Q as Fetch<'x>>::Prepared as crate::fetch::PreparedFetch<'y>>::Chunk: Send,
    for<'x> Func: Fn(<Q as FetchItem<'x>>::Item) + Send + Sync,
{
    fn execute(&mut self, mut data: (QueryData<Q, F>,)) {
        let mut borrow = data.0.borrow();
        borrow
            .iter_batched()
            .par_bridge()
            .for_each(|v| v.for_each(&self.func));
    }
}

impl<Q, F> SystemBuilder<(Query<Q, F>,)>
where
    for<'x> Q: Fetch<'x> + 'static,
    for<'x> F: Fetch<'x> + 'static,
{
    /// Execute a function for each item in the query
    pub fn for_each<Func>(self, func: Func) -> System<ForEach<Func>, (Query<Q, F>,), ()>
    where
        for<'x> Func: FnMut(<Q as FetchItem<'x>>::Item),
    {
        System::new(
            self.name.unwrap_or_else(|| type_name::<Func>().to_string()),
            ForEach { func },
            self.args,
        )
    }
}

#[cfg(feature = "rayon")]
impl<Q, F> SystemBuilder<(Query<Q, F>,)>
where
    for<'x> Q: 'static + Fetch<'x> + Send,
    for<'x> F: 'static + Fetch<'x> + Send,
    for<'x> <<Q as Fetch<'x>>::Prepared as crate::fetch::PreparedFetch<'x>>::Chunk: Send,
    // for<'x, 'y> crate::query::Batch<'y, <Q as Fetch<'x>>::Prepared>: Send,
{
    /// Execute a function for each item in the query in parallel batches
    pub fn par_for_each<Func>(self, func: Func) -> System<ParForEach<Func>, (Query<Q, F>,), ()>
    where
        for<'x> Func: Fn(<Q as FetchItem<'x>>::Item) + Send + Sync,
    {
        System::new(
            self.name.unwrap_or_else(|| type_name::<Func>().to_string()),
            ParForEach { func },
            self.args,
        )
    }
}

impl<Args> SystemBuilder<Args> {
    /// Use a query within the system
    ///
    /// Systems are automatically multithreaded on an archetype and component granularity level.
    ///
    /// This means that queries which access the same component mutably for *different* archetypes
    /// will be scheduled in parallel, and queries which access *different* components mutably in
    /// the same archetype will also be scheduled in parallel.
    pub fn with_query<Q, F, S>(self, query: Query<Q, F, S>) -> SystemBuilder<Args::PushRight>
    where
        Q: 'static + for<'x> Fetch<'x>,
        F: 'static + for<'x> Fetch<'x>,
        S: 'static + for<'x> QueryStrategy<'x, Q, F>,
        Args: TuplePush<Query<Q, F, S>>,
    {
        self.with(query)
    }
    /// Access the world
    ///
    /// **Note**: This still creates a barrier to queries in other systems as the archetypes can be
    /// mutated by a shared reference
    pub fn with_world(self) -> SystemBuilder<Args::PushRight>
    where
        Args: TuplePush<WithWorld>,
    {
        self.with(WithWorld)
    }

    /// Access the world mutably
    pub fn with_world_mut(self) -> SystemBuilder<Args::PushRight>
    where
        Args: TuplePush<WithWorldMut>,
    {
        self.with(WithWorldMut)
    }

    /// Access the command buffer
    pub fn with_cmd(self) -> SystemBuilder<Args::PushRight>
    where
        Args: TuplePush<WithCmd>,
    {
        self.with(WithCmd)
    }

    /// Access the command buffer mutably
    /// **Note**: Add `.flush()` after the system in the schedule to have the changes visible in
    /// the next system
    pub fn with_cmd_mut(self) -> SystemBuilder<Args::PushRight>
    where
        Args: TuplePush<WithCmdMut>,
    {
        self.with(WithCmdMut)
    }

    /// Access schedule input
    pub fn with_input<T>(self) -> SystemBuilder<Args::PushRight>
    where
        T: 'static,
        Args: TuplePush<WithInput<T>>,
    {
        self.with(WithInput::<T>(PhantomData))
    }

    /// Access schedule input mutably
    pub fn with_input_mut<T>(self) -> SystemBuilder<Args::PushRight>
    where
        T: 'static,
        Args: TuplePush<WithInputMut<T>>,
    {
        self.with(WithInputMut::<T>(PhantomData))
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
    pub fn with_resource<R>(self, resource: SharedResource<R>) -> SystemBuilder<Args::PushRight>
    where
        Args: TuplePush<SharedResource<R>>,
        R: Send + 'static,
    {
        self.with(resource)
    }

    /// Build the system by supplying a function to act upon the systems arguments,
    pub fn build<Func, Ret>(self, func: Func) -> System<Func, Args, Ret>
    where
        Args: for<'a> SystemData<'a> + 'static,
        Func: for<'this, 'a> SystemFn<'this, <Args as SystemData<'a>>::Value, Ret>,
    {
        System::new(
            self.name.unwrap_or_else(|| type_name::<Func>().to_string()),
            func,
            self.args,
        )
    }

    /// Add a new generic argument to the system
    fn with<S>(self, other: S) -> SystemBuilder<Args::PushRight>
    where
        S: for<'x> SystemData<'x>,
        Args: TuplePush<S>,
    {
        SystemBuilder {
            name: self.name,
            args: self.args.push_right(other),
        }
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

/// Abstraction over a system with any kind of arguments and fallibility
#[doc(hidden)]
pub trait DynSystem {
    fn name(&self) -> &str;
    fn describe(&self, f: &mut Formatter<'_>) -> fmt::Result;
    fn execute(&mut self, ctx: &SystemContext<'_, '_, '_>) -> anyhow::Result<()>;
    fn access(&self, world: &World, dst: &mut Vec<Access>);
}

impl<F, Args, Err> DynSystem for System<F, Args, Result<(), Err>>
where
    Args: for<'x> SystemData<'x>,
    F: for<'x> SystemFn<'x, <Args as SystemData<'x>>::Value, Result<(), Err>>,
    Err: Into<anyhow::Error>,
{
    fn execute(&mut self, ctx: &SystemContext<'_, '_, '_>) -> anyhow::Result<()> {
        profile_function!(self.name());

        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!("system", name = self.name).entered();

        let data = self.data.acquire(ctx);

        let res: anyhow::Result<()> = self.func.execute(data).map_err(Into::into);
        if let Err(err) = res {
            return Err(err.context(format!("Failed to execute system: {:?}", self)));
        }

        Ok(())
    }

    fn describe(&self, f: &mut fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("fn ")?;
        f.write_str(&self.name)?;
        self.data.describe(f)?;
        f.write_str(" -> ")?;
        f.write_str(&tynm::type_name::<core::result::Result<(), Err>>())?;

        Ok(())
    }

    fn access(&self, world: &World, dst: &mut Vec<Access>) {
        self.data.access(world, dst)
    }

    fn name(&self) -> &str {
        &self.name
    }
}

impl<F, Args> DynSystem for System<F, Args, ()>
where
    Args: for<'x> SystemData<'x>,
    F: for<'x> SystemFn<'x, <Args as SystemData<'x>>::Value, ()>,
{
    fn execute(&mut self, ctx: &SystemContext<'_, '_, '_>) -> anyhow::Result<()> {
        profile_function!(self.name());

        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!("system", name = self.name).entered();

        let data = self.data.acquire(ctx);

        self.func.execute(data);

        Ok(())
    }

    fn describe(&self, f: &mut fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("fn ")?;
        f.write_str(&self.name)?;
        self.data.describe(f)?;

        Ok(())
    }

    fn access(&self, world: &World, dst: &mut Vec<Access>) {
        self.data.access(world, dst)
    }

    fn name(&self) -> &str {
        &self.name
    }
}

impl<F, Args, Ret> fmt::Debug for System<F, Args, Ret>
where
    Self: DynSystem,
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
        Self: DynSystem,
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
    pub fn run<'a>(&'a mut self, world: &'a mut World) -> Ret
    where
        Ret: 'static,
        for<'x> Args: SystemData<'x>,
        for<'x> F: SystemFn<'x, <Args as SystemData<'x>>::Value, Ret>,
    {
        self.run_with(world, &mut ())
    }
}

impl<F, Args, Ret> System<F, Args, Ret> {
    /// Run the system on the world. Any commands will be applied
    pub fn run_with<'a>(&mut self, world: &mut World, input: impl IntoInput<'a>) -> Ret
    where
        Ret: 'static,
        for<'x> Args: SystemData<'x>,
        for<'x> F: SystemFn<'x, <Args as SystemData<'x>>::Value, Ret>,
    {
        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!("run_on", name = self.name).entered();

        let mut cmd = CommandBuffer::new();
        let input = input.into_input();
        let ctx = SystemContext::new(world, &mut cmd, &input);

        let data = self.data.acquire(&ctx);

        let ret = self.func.execute(data);
        ctx.cmd_mut()
            .apply(&mut ctx.world.borrow_mut())
            .expect("Failed to apply commandbuffer");
        ret
    }
}

#[derive(Hash, Debug, Clone, Copy, PartialEq, Eq, Ord, PartialOrd)]
/// Describes a kind of access
pub enum AccessKind {
    /// Borrow a single component of an archetype
    Archetype {
        /// The archetype id
        id: ArchetypeId,
        /// The accessed component
        component: ComponentKey,
    },
    /// A unit struct works as a synchronization barrier
    External(TypeId),
    /// Borrow the whole world
    World,
    /// Borrow the commandbuffer
    CommandBuffer,
    /// Data supplied by user in the execution context
    Input(TypeId),
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

/// An access for a component in an archetype
#[derive(Default, Debug, Clone)]
#[allow(dead_code)]
struct ArchetypeAccess {
    arch: ArchetypeInfo,
    components: Vec<ComponentAccessInfo>,
    change_events: Vec<ComponentAccessInfo>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
struct ComponentAccessInfo {
    mutable: bool,
    name: &'static str,
    id: ComponentKey,
}

/// Human friendly system access
#[derive(Default, Debug, Clone)]
pub struct AccessInfo {
    archetypes: BTreeMap<ArchetypeId, ArchetypeAccess>,
    world: Option<bool>,
    cmd: Option<bool>,
    external: Vec<TypeId>,
    input: Vec<(TypeId, bool)>,
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

/// Transform accesses into a human friendly format
pub(crate) fn access_info(accesses: &[Access], world: &World) -> AccessInfo {
    let mut result = AccessInfo::default();
    for access in accesses {
        match access.kind {
            AccessKind::Archetype { id, component } => {
                let arch = world.archetypes.get(id);
                result
                    .archetypes
                    .entry(id)
                    .or_insert_with(|| ArchetypeAccess {
                        arch: arch.desc(),
                        ..Default::default()
                    })
                    .components
                    .push(ComponentAccessInfo {
                        mutable: access.mutable,
                        name: arch.component(component).unwrap().name(),
                        id: component,
                    })
            }
            AccessKind::External(ty) => result.external.push(ty),
            AccessKind::Input(ty) => {
                result.input.push((ty, access.mutable));
            }
            AccessKind::World => match result.world {
                Some(true) => result.world = Some(true),
                _ => result.world = Some(access.mutable),
            },
            AccessKind::CommandBuffer => match result.cmd {
                Some(true) => result.cmd = Some(true),
                _ => result.cmd = Some(access.mutable),
            },
        }
    }

    result
}

impl Access {
    /// Returns true it both accesses can coexist
    pub(crate) fn is_compatible_with(&self, other: &Self) -> bool {
        !(self.kind == other.kind && (self.mutable || other.mutable))
    }
}

/// A type erased system
pub struct BoxedSystem {
    inner: Box<dyn DynSystem + Send + Sync>,
}

impl core::fmt::Debug for BoxedSystem {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.inner.describe(f)
    }
}

impl BoxedSystem {
    /// Same as execute but creates and applied a temporary commandbuffer
    pub fn run<'a>(&'a mut self, world: &'a mut World) -> anyhow::Result<()> {
        self.run_with(world, &mut ())
    }
}

impl BoxedSystem {
    /// Creates a new boxed system from any other kind of system
    fn new<S>(system: S) -> Self
    where
        S: DynSystem + Send + Sync + 'static,
    {
        Self {
            inner: Box::new(system),
        }
    }

    /// Execute the system with the provided context
    pub fn execute<'a>(&'a mut self, ctx: &'a SystemContext<'_, '_, '_>) -> anyhow::Result<()> {
        self.inner.execute(ctx)
    }

    /// Same as execute but creates and applied a temporary command buffer
    pub fn run_with<'a>(
        &'a mut self,
        world: &'a mut World,
        input: impl IntoInput<'a>,
    ) -> anyhow::Result<()> {
        let mut cmd = CommandBuffer::new();
        let input = input.into_input();
        let ctx = SystemContext::new(world, &mut cmd, &input);
        self.inner.execute(&ctx)?;

        ctx.cmd_mut()
            .apply(&mut ctx.world.borrow_mut())
            .expect("Failed to apply commandbuffer");

        Ok(())
    }

    /// Describes the system held within
    pub fn describe(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.inner.describe(f)
    }

    /// Returns the accesses of the system held within
    pub fn access(&self, world: &World, dst: &mut Vec<Access>) {
        self.inner.access(world, dst)
    }

    /// Returns the boxed system's name
    pub fn name(&self) -> &str {
        self.inner.name()
    }
}

impl<T> From<T> for BoxedSystem
where
    T: 'static + Send + Sync + DynSystem,
{
    fn from(system: T) -> Self {
        Self::new(system)
    }
}

#[cfg(test)]
#[cfg(feature = "std")]
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
            .build(move |mut query: QueryBorrow<_>| -> anyhow::Result<()> {
                // Lock archetypes
                let item: &i32 = query.get(id)?;
                eprintln!("Item: {item}");

                Ok(())
            });

        let mut cmd = CommandBuffer::new();

        #[allow(clippy::let_unit_value)]
        let data = &mut ();
        let ctx = SystemContext::new(&mut world, &mut cmd, data);

        system.execute(&ctx).unwrap();

        fallible.execute(&ctx).unwrap();

        world.remove(id, b()).unwrap();

        let mut boxed = fallible.boxed();

        let ctx = SystemContext::new(&mut world, &mut cmd, data);
        let res = boxed.execute(&ctx);
        let _ = res.unwrap_err();
    }

    #[test]
    fn system_builder_empty() {
        let mut a = 5;
        let mut system = System::builder().build(|| {
            a += 1;
        });

        let mut world = World::new();
        system.run(&mut world);

        assert_eq!(a, 6);
    }
}
