use core::{
    fmt::{self, Formatter},
    marker::PhantomData,
};

use atomic_refcell::{AtomicRef, AtomicRefCell, AtomicRefMut};

use crate::{
    query::QueryStrategy,
    system::{
        traits::{WithCmd, WithCmdMut, WithWorld},
        AsBorrowed, CallableVariadic,
    },
    util::TuplePush,
    CommandBuffer, Entity, EntityRef, Fetch, Query, World,
};

/// Allows direct communication with an entity as a system.
///
/// A signal allows the same functionality and access as a normal [`System`], but runs on a single
/// specified entity.
pub struct Signal<F, T, Args, Ret> {
    name: String,
    data: Args,
    func: F,
    _marker: PhantomData<(T, Ret)>,
}

impl<T> Signal<(), T, (), ()> {
    /// Creates a new [`SignalBuilder`]
    pub fn builder(name: impl Into<String>) -> SignalBuilder<T, (ExtractEntity,)> {
        SignalBuilder::new(name)
    }
}

impl<F, T, Args, Ret> Signal<F, T, Args, Ret> {
    fn new(name: impl Into<String>, data: Args, func: F) -> Self {
        Self {
            name: name.into(),
            data,
            func,
            _marker: PhantomData,
        }
    }
}

impl<F, T, Args, Err> DynSignal<T> for Signal<F, T, Args, Result<(), Err>>
where
    F: 'static
        + Send
        + Sync
        + for<'x, 'y> CallableVariadic<
            <<<Args as SignalData<'x>>::Value as AsBorrowed<'y>>::Borrowed as TuplePush<T>>::PushRight,
            Result<(), Err>,
        >,
    Args: 'static + Send + Sync + for<'x> SignalData<'x>,
    for<'x, 'y> <<Args as SignalData<'x>>::Value as AsBorrowed<'y>>::Borrowed: TuplePush<T>,
    Err: 'static + Send + Sync + Into<anyhow::Error>,
    T: 'static + Send + Sync,
{
    /// Process the signal on the entity
    fn execute_with_context(&mut self, ctx: &SignalContext<'_>, arg: T) -> anyhow::Result<()> {
        profile_function!(self.name());

        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!("signal", name = self.name).entered();

        let res: anyhow::Result<()>;
        {
            let mut data = self.data.acquire(ctx);

            let data = data.as_borrowed();
            res = self.func.execute(data.push_right(arg)).map_err(Into::into);
        }

        if let Err(err) = res {
            return Err(err.context(format!("Failed to execute signal: {:?}", self.name())));
        }

        Ok(())
    }

    /// Returns the signals name
    fn name(&self) -> &str {
        &self.name
    }
}

/// Builder for [`Signal`]
pub struct SignalBuilder<T, Args> {
    name: String,
    data: Args,
    _marker: PhantomData<T>,
}

impl<T> SignalBuilder<T, (ExtractEntity,)> {
    /// Creates a new builder for a [`Signal`].
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            data: (ExtractEntity,),
            _marker: PhantomData,
        }
    }
}

impl<T, Args> SignalBuilder<T, Args> {
    /// Use a query within the signal
    pub fn with_query<Q, F, S>(self, query: Query<Q, F, S>) -> SignalBuilder<T, Args::PushRight>
    where
        Q: 'static + for<'x> Fetch<'x>,
        F: 'static + for<'x> Fetch<'x>,
        S: 'static + for<'x> QueryStrategy<'x, Q, F>,
        Args: TuplePush<Query<Q, F, S>>,
    {
        self.with(query)
    }

    /// Access the world
    pub fn with_world(self) -> SignalBuilder<T, Args::PushRight>
    where
        Args: TuplePush<WithWorld>,
    {
        self.with(WithWorld)
    }

    /// Access the commandbuffer
    pub fn with_cmd(self) -> SignalBuilder<T, Args::PushRight>
    where
        Args: TuplePush<WithCmd>,
    {
        self.with(WithCmd)
    }

    /// Access the commandbuffer
    pub fn with_cmd_mut(self) -> SignalBuilder<T, Args::PushRight>
    where
        Args: TuplePush<WithCmdMut>,
    {
        self.with(WithCmdMut)
    }

    /// Add a new generic argument to the system.
    /// See: [Self::with_query], [Self::with_world] etc for non-generic shorthands
    pub fn with<S>(self, other: S) -> SignalBuilder<T, Args::PushRight>
    where
        S: for<'x> SignalData<'x>,
        Args: TuplePush<S>,
    {
        SignalBuilder {
            name: self.name,
            data: self.data.push_right(other),
            _marker: self._marker,
        }
    }

    /// Finish building the signal
    pub fn build<F, Ret>(self, func: F) -> Signal<F, T, Args, Ret>
    where
        Args: for<'a> SignalData<'a> + 'static,
        for<'x, 'y> <<Args as SignalData<'x>>::Value as AsBorrowed<'y>>::Borrowed: TuplePush<T>,
        F: 'static + Send + Sync +
         for<'x, 'y> CallableVariadic< <<<Args as SignalData<'x>>::Value as AsBorrowed<'y>>::Borrowed as TuplePush<T>>::PushRight ,Ret>,
    {
        Signal::new(self.name, self.data, func)
    }
}

/// Type erased signal
pub type BoxedSignal<T = ()> = Box<dyn DynSignal<T>>;

/// Type erased signal
pub trait DynSignal<T>: 'static + Send + Sync {
    /// Debug name of the signal
    fn name(&self) -> &str;

    /// Execute the signal on the specified entity
    fn execute_with_context(&mut self, ctx: &SignalContext<'_>, arg: T) -> anyhow::Result<()>;

    /// Execute the signal on the specified entity
    fn execute(
        &mut self,
        entity: EntityRef,
        cmd: &mut CommandBuffer,
        arg: T,
    ) -> anyhow::Result<()> {
        self.execute_with_context(&SignalContext::new(entity, cmd), arg)
    }

    /// Erase the signals type
    fn boxed(self) -> BoxedSignal<T>
    where
        Self: Sized,
    {
        Box::new(self)
    }
}

/// Borrow state from the system execution data
pub trait SignalData<'a> {
    /// The borrow from the system context
    type Value: for<'x> AsBorrowed<'x>;

    /// Get the data from the system context
    fn acquire(&'a mut self, ctx: &'a SignalContext<'_>) -> Self::Value;
    /// Human friendly debug description
    fn describe(&self, f: &mut Formatter<'_>) -> fmt::Result;
}

/// Everything needed to invoke a signal
pub struct SignalContext<'w> {
    entity: EntityRef<'w>,
    world: AtomicRefCell<&'w World>,
    cmd: AtomicRefCell<&'w mut CommandBuffer>,
}

impl<'a> SignalContext<'a> {
    /// Creates a new system context
    pub fn new(entity: EntityRef<'a>, cmd: &'a mut CommandBuffer) -> Self {
        Self {
            entity,
            world: AtomicRefCell::new(entity.world()),
            cmd: AtomicRefCell::new(cmd),
        }
    }

    /// Access the world
    #[inline]
    pub fn world(&self) -> AtomicRef<World> {
        let borrow = self.world.borrow();
        AtomicRef::map(borrow, |v| *v)
    }

    /// Access the commandbuffer
    #[inline]
    pub fn cmd(&self) -> AtomicRef<CommandBuffer> {
        let borrow = self.cmd.borrow();
        AtomicRef::map(borrow, |v| *v)
    }

    /// Access the commandbuffer mutably
    #[inline]
    pub fn cmd_mut(&self) -> AtomicRefMut<CommandBuffer> {
        let borrow = self.cmd.borrow_mut();
        AtomicRefMut::map(borrow, |v| *v)
    }
}

impl<'a> SignalData<'a> for WithWorld {
    type Value = AtomicRef<'a, World>;

    fn acquire(&'a mut self, ctx: &'a SignalContext<'_>) -> Self::Value {
        ctx.world()
    }

    fn describe(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str("World")
    }
}

impl<'a> SignalData<'a> for WithCmd {
    type Value = AtomicRef<'a, CommandBuffer>;

    fn acquire(&'a mut self, ctx: &'a SignalContext<'_>) -> Self::Value {
        ctx.cmd()
    }

    fn describe(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str("Cmd")
    }
}

impl<'a> SignalData<'a> for WithCmdMut {
    type Value = AtomicRefMut<'a, CommandBuffer>;

    fn acquire(&'a mut self, ctx: &'a SignalContext<'_>) -> Self::Value {
        ctx.cmd_mut()
    }

    fn describe(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str("Cmd")
    }
}

/// Extracts the target entity of the signal
pub struct ExtractEntity;

impl<'a> SignalData<'a> for ExtractEntity {
    type Value = Entity;

    fn acquire(&'a mut self, ctx: &'a SignalContext<'_>) -> Self::Value {
        ctx.entity.id()
    }

    fn describe(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str("entity")
    }
}

// /// Extracts the target entity ref of the signal
// pub struct ExtractEntityRef;

// impl<'a> SignalData<'a> for ExtractEntityRef {
//     type Value = EntityRef<'a>;

//     fn acquire(&'a mut self, ctx: &'a SignalContext<'_>) -> Self::Value {
//         ctx.entity
//     }

//     fn describe(&self, f: &mut Formatter<'_>) -> fmt::Result {
//         f.write_str("entity")
//     }
// }

struct FmtSignalData<'a, S>(&'a S);
impl<'w, S> core::fmt::Debug for FmtSignalData<'_, S>
where
    S: SignalData<'w>,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        self.0.describe(f)
    }
}
macro_rules! tuple_impl {
    ($($idx: tt => $ty: ident),*) => {

        impl<'a, $($ty,)*> SignalData<'a> for ($($ty,)*)
        where
            $($ty: SignalData<'a>,)*
        {
            type Value = ($(<$ty as SignalData<'a>>::Value,)*);

            #[allow(clippy::unused_unit)]
            fn acquire(&'a mut self, _ctx: &'a SignalContext<'_>) -> Self::Value {
                ($((self.$idx).acquire(_ctx),)*)
            }

            fn describe(&self, f: &mut Formatter<'_>) -> fmt::Result {
                core::fmt::Debug::fmt(&($(
                    FmtSignalData(&self.$idx),
                )*), f)

            }
        }
    };
}

tuple_impl! {}
tuple_impl! { 0 => A }
tuple_impl! { 0 => A, 1 => B }
tuple_impl! { 0 => A, 1 => B, 2 => C }
tuple_impl! { 0 => A, 1 => B, 2 => C, 3 => D }
tuple_impl! { 0 => A, 1 => B, 2 => C, 3 => D, 4 => E }
tuple_impl! { 0 => A, 1 => B, 2 => C, 3 => D, 4 => E, 5 => F }
tuple_impl! { 0 => A, 1 => B, 2 => C, 3 => D, 4 => E, 5 => F, 6 => H }

#[cfg(test)]
mod test {
    use core::sync::atomic::AtomicBool;
    use std::sync::Arc;

    use crate::{system::traits::WithWorld, CommandBuffer, Entity, World};

    use super::{BoxedSignal, DynSignal, ExtractEntity, Signal, SignalContext};

    #[test]
    fn basic_signal() {
        let called = Arc::new(AtomicBool::new(false));
        let mut signal: BoxedSignal<()> = Signal::builder("test")
            .with_world()
            .build(move |_id, _: &World, ()| {
                called.store(true, core::sync::atomic::Ordering::SeqCst);

                anyhow::Ok(())
            })
            .boxed();

        let mut world = World::new();

        let id = world.spawn();
        signal
            .execute_with_context(
                &SignalContext::new(world.entity(id).unwrap(), &mut CommandBuffer::new()),
                (),
            )
            .unwrap();
    }
}
