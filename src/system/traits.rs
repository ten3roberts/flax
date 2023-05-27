use alloc::{string::String, vec::Vec};
use atomic_refcell::{AtomicRef, AtomicRefMut};
use core::{
    fmt::{self, Formatter},
    marker::PhantomData,
};

use crate::system::AccessKind;
use crate::*;

use super::{Access, SystemContext};

/// Allows dereferencing `AtomicRef<T>` to &T and similar "lock" types in a safe manner.
/// Traits for guarded types like `AtomicRef`, `Mutex` or [`QueryData`](crate::QueryData).
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

struct FmtSystemData<'a, S, T>(&'a S, PhantomData<T>);
impl<'a, 'w, S, T> core::fmt::Debug for FmtSystemData<'a, S, T>
where
    S: SystemData<'w, T>,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        self.0.describe(f)
    }
}

/// Provider trait for data from a system execution context
pub trait SystemData<'a, T>: SystemAccess {
    /// The borrow from the system context
    type Value;
    /// Get the data from the system context
    fn acquire(&'a mut self, ctx: &'a SystemContext<'_, T>) -> Self::Value;
    /// Human friendly debug description
    fn describe(&self, f: &mut Formatter<'_>) -> fmt::Result;
}

/// Describe an access to the world in terms of shared and unique accesses
pub trait SystemAccess {
    /// Returns all the accesses for a system
    fn access(&self, world: &World, dst: &mut Vec<Access>);
}

/// A callable function
pub trait SystemFn<'this, Args, Ret> {
    /// Execute the function
    fn execute(&'this mut self, args: Args) -> Ret;
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
            for<'x> Func: FnMut($(<$ty as AsBorrow<'x>>::Borrowed),*) -> Ret,
        {
            fn execute(&'this mut self, mut args: ($($ty,)*)) -> Ret {
                let borrowed = ($(args.$idx.as_borrow(),)*);
                (self)($(borrowed.$idx,)*)
            }
        }

        impl<$($ty,)*> SystemAccess for ($($ty,)*)
        where
            $($ty: SystemAccess,)*
        {
            fn access(&self, world: &World, dst: &mut Vec<Access>) {
                $(self.$idx.access(world, dst);)*
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

        impl<'w, $($ty,)* T> SystemData<'w, T> for ($($ty,)*)
        where
            $($ty: SystemData<'w, T>,)*
        {
            type Value = ($(<$ty as SystemData<'w, T>>::Value,)*);

            fn acquire(&'w mut self, _ctx: &'w SystemContext<'_, T>) -> Self::Value {
                ($((self.$idx).acquire(_ctx),)*)
            }

            fn describe(&self, f: &mut Formatter<'_>) -> fmt::Result {
                core::fmt::Debug::fmt(&($(
                    FmtSystemData(&self.$idx, PhantomData),
                )*), f)

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
pub struct Write<T>(pub(crate) PhantomData<T>);
#[doc(hidden)]
pub struct Read<T>(pub(crate) PhantomData<T>);

impl<'a, T> SystemData<'a, T> for Write<World> {
    type Value = AtomicRefMut<'a, World>;

    fn acquire(&mut self, ctx: &'a SystemContext<'_, T>) -> Self::Value {
        ctx.world_mut()
    }

    fn describe(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str("&mut World")
    }
}

impl<'a, T> SystemData<'a, T> for Read<World> {
    type Value = AtomicRef<'a, World>;

    fn acquire(&mut self, ctx: &'a SystemContext<'_, T>) -> Self::Value {
        ctx.world()
    }

    fn describe(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str("&World")
    }
}

impl SystemAccess for Write<World> {
    fn access(&self, _: &World, dst: &mut Vec<Access>) {
        dst.push(Access {
            kind: AccessKind::World,
            mutable: true,
        });
    }
}

impl SystemAccess for Read<World> {
    fn access(&self, _: &World, dst: &mut Vec<Access>) {
        dst.push(Access {
            kind: AccessKind::World,
            // Due to interior mutablity as anything can be borrowed mut
            mutable: true,
        });
    }
}

impl<'a, T> SystemData<'a, T> for Write<CommandBuffer> {
    type Value = AtomicRefMut<'a, CommandBuffer>;

    fn acquire(&mut self, ctx: &'a SystemContext<'_, T>) -> Self::Value {
        ctx.cmd_mut()
    }

    fn describe(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str("&mut CommandBuffer")
    }
}

impl SystemAccess for Write<CommandBuffer> {
    fn access(&self, _: &World, dst: &mut Vec<Access>) {
        dst.push(Access {
            kind: AccessKind::CommandBuffer,
            mutable: true,
        });
    }
}

#[doc(hidden)]
pub struct ReadContextData<T>(pub(crate) PhantomData<T>);

impl<'a, T: 'a> SystemData<'a, T> for ReadContextData<T> {
    type Value = AtomicRef<'a, T>;

    fn acquire(&'a mut self, ctx: &'a SystemContext<'_, T>) -> Self::Value {
        ctx.data()
    }

    fn describe(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str("&")?;
        f.write_str(&tynm::type_name::<T>())
    }
}

impl<T> SystemAccess for ReadContextData<T> {
    fn access(&self, _: &World, dst: &mut Vec<Access>) {
        dst.push(Access {
            kind: AccessKind::ContextData,
            mutable: false,
        });
    }
}

#[doc(hidden)]
pub struct WriteContextData<T>(pub(crate) PhantomData<T>);

impl<'a, T: 'a> SystemData<'a, T> for WriteContextData<T> {
    type Value = AtomicRefMut<'a, T>;

    fn acquire(&'a mut self, ctx: &'a SystemContext<'_, T>) -> Self::Value {
        ctx.data_mut()
    }

    fn describe(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str("&mut ")?;
        f.write_str(&tynm::type_name::<T>())
    }
}

impl<T> SystemAccess for WriteContextData<T> {
    fn access(&self, _: &World, dst: &mut Vec<Access>) {
        dst.push(Access {
            kind: AccessKind::ContextData,
            mutable: true,
        });
    }
}

#[cfg(test)]
mod test {

    use core::marker::PhantomData;

    use alloc::string::String;
    use atomic_refcell::AtomicRefMut;
    use itertools::Itertools;

    use crate::{
        component, components::name, system::SystemContext, All, CommandBuffer, Component, Entity,
        Query, QueryBorrow, QueryData, World,
    };

    use super::{SystemData, SystemFn, Write};

    component! {
        health: f32,
    }

    #[test]
    fn system_fn() -> anyhow::Result<()> {
        let mut world = World::new();
        let mut cmd = CommandBuffer::new();
        #[allow(clippy::let_unit_value)]
        let mut data = ();
        let ctx = SystemContext::new(&mut world, &mut cmd, &mut data);

        let mut spawner = |w: &mut World| {
            Entity::builder()
                .set(name(), "Neo".into())
                .set(health(), 90.0)
                .spawn(w);

            Entity::builder()
                .set(name(), "Trinity".into())
                .set(health(), 85.0)
                .spawn(w);
        };

        let mut reader = |mut q: QueryBorrow<Component<String>, All>| {
            let names = q.iter().cloned().sorted().collect_vec();

            assert_eq!(names, ["Neo", "Trinity"]);
        };

        let data = &mut (Write::<World>(PhantomData),);
        let data: (AtomicRefMut<World>,) = data.acquire(&ctx);
        SystemFn::<(AtomicRefMut<World>,), ()>::execute(&mut spawner, data);
        // (spawner).execute(data);

        let data = &mut (Query::new(name()),);
        let data = data.acquire(&ctx);
        SystemFn::<(QueryData<_>,), ()>::execute(&mut reader, data);
        Ok(())
    }
}
