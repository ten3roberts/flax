use std::{
    fmt::{self, Formatter},
    marker::PhantomData,
};

use atomic_refcell::{AtomicRef, AtomicRefMut};
use eyre::eyre;

use crate::*;

/// Allows dereferencing `AtomicRef<T>` to &T and similar "lock" types in a safe manner.
pub trait AsBorrow<'a> {
    /// The dereference target
    type Borrowed: 'a;

    /// Dereference a held borrow
    fn as_borrow(&'a mut self) -> Self::Borrowed;
}

impl<'a, 'b, T: 'a> AsBorrow<'a> for AtomicRef<'b, T> {
    type Borrowed = &'a T;

    fn as_borrow(&'a mut self) -> Self::Borrowed {
        &*self
    }
}

impl<'a, 'b, T: 'a> AsBorrow<'a> for AtomicRefMut<'b, T> {
    type Borrowed = &'a mut T;

    fn as_borrow(&'a mut self) -> Self::Borrowed {
        &mut *self
    }
}

impl<'a, 'w, Q, F> AsBorrow<'a> for QueryData<'w, Q, F>
where
    Q: for<'x> Fetch<'x> + 'static,
    F: for<'x> Filter<'x> + 'static,
{
    type Borrowed = QueryBorrow<'a, Q, F>;

    fn as_borrow(&'a mut self) -> Self::Borrowed {
        self.borrow()
    }
}

/// Describes a type which can fetch assocated Data from the system context and
/// provide it to the system.
pub trait SystemData<'a>: SystemAccess {
    /// The borrow from the system context
    type Value;
    /// Get the data from the system context
    fn acquire(&'a mut self, ctx: &'a SystemContext<'_>) -> eyre::Result<Self::Value>;
}

/// Describe an access to the world in ters of shared and unique accesses
pub trait SystemAccess {
    /// Returns all the accesses for a system
    fn access(&self, world: &World) -> Vec<Access>;
}

/// A callable function
pub trait SystemFn<'this, Args, Ret> {
    /// Execute the function
    fn execute(&'this mut self, args: Args) -> Ret;
    /// Debug for Fn
    fn describe(&self, f: &mut Formatter<'_>) -> fmt::Result;
    /// Returns a short name
    fn name(&self) -> String;
    /// Returns the data accesses of a system function
    fn access(&self, world: &World) -> Vec<Access>;
}

#[derive(PartialEq, Eq, Clone)]
pub(crate) struct Verbatim(pub String);
impl fmt::Debug for Verbatim {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

macro_rules! tuple_impl {
    ($($idx: tt => $ty: ident),*) => {
        impl<'this, Func, Ret, $($ty,)*> SystemFn<'this, ($($ty,)*), Ret> for Func
        where
            $(for<'x> $ty: AsBorrow<'x>,)*
            // for<'x> Func: FnMut($($ty),*) -> Ret,
            for<'x> Func: FnMut($(<$ty as AsBorrow<'x>>::Borrowed),*) -> Ret,
        {
            fn execute(&'this mut self, mut args: ($($ty,)*)) -> Ret {
                let borrowed = ($(args.$idx.as_borrow(),)*);
                (self)($(borrowed.$idx,)*)
            }

            fn describe(&self, f: &mut Formatter<'_>) -> fmt::Result {
                use std::fmt::Debug;
                f.write_str("fn")?;

                ($(
                    Verbatim(tynm::type_name::<<$ty as AsBorrow>::Borrowed>()),
                )*).fmt(f)?;

                if std::any::type_name::<Ret>() != std::any::type_name::<()>() {
                    write!(f, " -> {}", tynm::type_name::<Ret>())?;
                }

                Ok(())
            }

            fn access(&self, _: &World) -> Vec<Access> {
                vec![]
            }

            fn name(&self) -> String {
                std::any::type_name::<Self>().to_string()
            }
        }

        impl<$($ty,)*> SystemAccess for ($($ty,)*)
        where
            $($ty: SystemAccess,)*
        {
            fn access(&self, world: &World) -> Vec<Access> {
                [
                    $(self.$idx.access(&*world)),*
                ].concat()
            }
        }

        impl<'a, $($ty,)*> AsBorrow<'a> for ($($ty,)*)
        where
            $($ty: AsBorrow<'a>,)*
        {
            type Borrowed = ($(<$ty as AsBorrow<'a>>::Borrowed,)*);

            fn as_borrow(&'a mut self) -> Self::Borrowed {
                ($((self.$idx).as_borrow(),)*)
            }
        }

        impl<'w, $($ty,)*> SystemData<'w> for ($($ty,)*)
        where
            $($ty: SystemData<'w>,)*
        {
            type Value = ($(<$ty as SystemData<'w>>::Value,)*);

            fn acquire(&'w mut self, _ctx: &'w SystemContext<'_>) -> eyre::Result<Self::Value> {
                Ok(
                    ($((self.$idx).acquire(_ctx)?,)*)
                )
            }
        }
    };
}

// tuple_impl! {}
tuple_impl! { 0 => A }
tuple_impl! { 0 => A, 1 => B }
tuple_impl! { 0 => A, 1 => B, 2 => C }
tuple_impl! { 0 => A, 1 => B, 2 => C, 3 => D }
tuple_impl! { 0 => A, 1 => B, 2 => C, 3 => D, 4 => E }
tuple_impl! { 0 => A, 1 => B, 2 => C, 3 => D, 4 => E, 5 => F }
tuple_impl! { 0 => A, 1 => B, 2 => C, 3 => D, 4 => E, 5 => F, 6 => H }
// tuple_impl! { 0 => A, 1 => B, 2 => C, 3 => D, 4 => E, 5 => F, 6 => H, 7 => I }
// tuple_impl! { 0 => A, 1 => B, 2 => C, 3 => D, 4 => E, 5 => F, 6 => H, 7 => I, 8 => J }

// pub trait SystemData<'init, 'ctx, 'w> {
//     type Init;
//     /// Initialize and fetch data from the system execution context
//     fn init(ctx: &'ctx SystemContext<'w>) -> Self::Init;
// }

/// Access part of the context mutably.
#[doc(hidden)]
pub struct Write<T>(PhantomData<T>);
#[doc(hidden)]
pub struct Read<T>(PhantomData<T>);

impl<T> Write<T> {
    pub(crate) fn new() -> Self {
        Self(PhantomData)
    }
}

impl<T> Read<T> {
    pub(crate) fn new() -> Self {
        Self(PhantomData)
    }
}

impl<T> Default for Read<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Default for Write<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> SystemData<'a> for Write<World> {
    type Value = AtomicRefMut<'a, World>;

    fn acquire(&mut self, ctx: &'a SystemContext<'_>) -> eyre::Result<Self::Value> {
        ctx.world_mut()
            .map_err(|_| eyre!("Failed to borrow world mutably"))
    }
}

impl<'a> SystemData<'a> for Read<World> {
    type Value = AtomicRef<'a, World>;

    fn acquire(&mut self, ctx: &'a SystemContext<'_>) -> eyre::Result<Self::Value> {
        ctx.world()
            .map_err(|_| eyre!("Failed to borrow world mutably"))
    }
}

impl SystemAccess for Write<World> {
    fn access(&self, _: &World) -> Vec<Access> {
        vec![Access {
            kind: AccessKind::World,
            mutable: true,
        }]
    }
}

impl SystemAccess for Read<World> {
    fn access(&self, _: &World) -> Vec<Access> {
        vec![Access {
            kind: AccessKind::World,
            mutable: true, // Due to interior mutablity as anything can be
                           // borrowed mut
        }]
    }
}

impl<'a> SystemData<'a> for Write<CommandBuffer> {
    type Value = AtomicRefMut<'a, CommandBuffer>;

    fn acquire(&mut self, ctx: &'a SystemContext<'_>) -> eyre::Result<Self::Value> {
        ctx.cmd_mut()
            .map_err(|_| eyre!("Failed to borrow commandbuffer mutably"))
    }
}

impl SystemAccess for Write<CommandBuffer> {
    fn access(&self, _: &World) -> Vec<Access> {
        vec![Access {
            kind: AccessKind::CommandBuffer,
            mutable: true,
        }]
    }
}

#[cfg(test)]
mod test {

    use atomic_refcell::AtomicRefMut;
    use itertools::Itertools;

    use crate::{
        component, components::name, system::SystemContext, All, CommandBuffer, Component, Entity,
        Query, QueryBorrow, QueryData, SystemData, SystemFn, World,
    };

    use super::Write;

    component! {
        health: f32,
    }

    #[test]
    fn system_fn() -> eyre::Result<()> {
        let mut world = World::new();
        let mut cmd = CommandBuffer::new();
        let ctx = SystemContext::new(&mut world, &mut cmd);

        let mut spawner = |w: &mut World| {
            Entity::builder()
                .set(name(), "Neo".to_string())
                .set(health(), 90.0)
                .spawn(w);

            Entity::builder()
                .set(name(), "Trinity".to_string())
                .set(health(), 85.0)
                .spawn(w);
        };

        let mut reader = |mut q: QueryBorrow<Component<String>, All>| {
            let names = q.iter().cloned().sorted().collect_vec();

            assert_eq!(names, ["Neo", "Trinity"]);
        };

        let data = &mut (Write::<World>::new(),);
        let data: (AtomicRefMut<World>,) = data.acquire(&ctx).unwrap();
        SystemFn::<(AtomicRefMut<World>,), ()>::execute(&mut spawner, data);
        // (spawner).execute(data);

        let data = &mut (Query::new(name()),);
        let data = data.acquire(&ctx).unwrap();
        SystemFn::<(QueryData<_>,), ()>::execute(&mut reader, data);
        Ok(())
    }
}
