use alloc::vec::Vec;
use atomic_refcell::{AtomicRef, AtomicRefMut};
use core::{
    any::TypeId,
    fmt::{self, Formatter},
    marker::PhantomData,
};

use crate::system::AccessKind;
use crate::*;

use super::{Access, SystemContext};

/// Allows dereferencing `AtomicRef<T>` to &T and similar "lock" types in a safe manner.
/// Traits for guarded types like `AtomicRef`, `Mutex` or [`QueryData`](crate::query::QueryData).
pub trait AsBorrowed<'a> {
    /// The dereference target
    type Borrowed: 'a;

    /// Dereference a held borrow
    fn as_borrowed(&'a mut self) -> Self::Borrowed;
}

impl<'a, 'b, T: 'a> AsBorrowed<'a> for AtomicRef<'b, T> {
    type Borrowed = &'a T;

    fn as_borrowed(&'a mut self) -> Self::Borrowed {
        &*self
    }
}

impl<'a, 'b, T: 'a> AsBorrowed<'a> for AtomicRefMut<'b, T> {
    type Borrowed = &'a mut T;

    fn as_borrowed(&'a mut self) -> Self::Borrowed {
        &mut *self
    }
}

struct FmtSystemData<'a, S>(&'a S);
impl<'a, 'w, S> core::fmt::Debug for FmtSystemData<'a, S>
where
    S: SystemData<'w>,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        self.0.describe(f)
    }
}

/// Borrow state from the system execution data
pub trait SystemData<'a>: SystemAccess {
    /// The borrow from the system context
    type Value;

    /// Get the data from the system context
    fn acquire(&'a mut self, ctx: &'a SystemContext<'_, '_, '_>) -> Self::Value;
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

macro_rules! tuple_impl {
    ($($idx: tt => $ty: ident),*) => {
        impl<'this, Func, Ret, $($ty,)*> SystemFn<'this, ($($ty,)*), Ret> for Func
        where
            $(for<'x> $ty: AsBorrowed<'x>,)*
            for<'x> Func: FnMut($(<$ty as AsBorrowed<'x>>::Borrowed),*) -> Ret,
        {
            fn execute(&'this mut self, mut _args: ($($ty,)*)) -> Ret {
                let _borrowed = ($(_args.$idx.as_borrowed(),)*);
                (self)($(_borrowed.$idx,)*)
            }
        }

        impl<$($ty,)*> SystemAccess for ($($ty,)*)
        where
            $($ty: SystemAccess,)*
        {
            fn access(&self, _world: &World, _dst: &mut Vec<Access>) {
                $(self.$idx.access(_world, _dst);)*
            }
        }

        impl<'a, $($ty,)*> SystemData<'a> for ($($ty,)*)
        where
            $($ty: SystemData<'a>,)*
        {
            type Value = ($(<$ty as SystemData<'a>>::Value,)*);

            #[allow(clippy::unused_unit)]
            fn acquire(&'a mut self, _ctx: &'a SystemContext<'_, '_, '_>) -> Self::Value {
                ($((self.$idx).acquire(_ctx),)*)
            }

            fn describe(&self, f: &mut Formatter<'_>) -> fmt::Result {
                core::fmt::Debug::fmt(&($(
                    FmtSystemData(&self.$idx),
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

/// Access the world
pub struct WithWorld;

impl<'a> SystemData<'a> for WithWorld {
    type Value = AtomicRef<'a, World>;

    fn acquire(&mut self, ctx: &'a SystemContext<'_, '_, '_>) -> Self::Value {
        ctx.world()
    }

    fn describe(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str("&World")
    }
}

impl SystemAccess for WithWorld {
    fn access(&self, _: &World, dst: &mut Vec<Access>) {
        dst.push(Access {
            kind: AccessKind::World,
            // Due to interior mutablity as anything can be borrowed mut
            mutable: true,
        });
    }
}

/// Access the world mutably
pub struct WithWorldMut;

impl<'a> SystemData<'a> for WithWorldMut {
    type Value = AtomicRefMut<'a, World>;

    fn acquire(&mut self, ctx: &'a SystemContext<'_, '_, '_>) -> Self::Value {
        ctx.world_mut()
    }

    fn describe(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str("&mut World")
    }
}

impl SystemAccess for WithWorldMut {
    fn access(&self, _: &World, dst: &mut Vec<Access>) {
        dst.push(Access {
            kind: AccessKind::World,
            // Due to interior mutablity as anything can be borrowed mut
            mutable: true,
        });
    }
}

/// Access the command buffer
pub struct WithCmd;

impl<'a> SystemData<'a> for WithCmd {
    type Value = AtomicRef<'a, CommandBuffer>;

    fn acquire(&mut self, ctx: &'a SystemContext<'_, '_, '_>) -> Self::Value {
        ctx.cmd()
    }

    fn describe(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str("&CommandBuffer")
    }
}

impl SystemAccess for WithCmd {
    fn access(&self, _: &World, dst: &mut Vec<Access>) {
        dst.push(Access {
            kind: AccessKind::CommandBuffer,
            mutable: false,
        });
    }
}

/// Access the command buffer mutably
pub struct WithCmdMut;

impl<'a> SystemData<'a> for WithCmdMut {
    type Value = AtomicRefMut<'a, CommandBuffer>;

    fn acquire(&mut self, ctx: &'a SystemContext<'_, '_, '_>) -> Self::Value {
        ctx.cmd_mut()
    }

    fn describe(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str("&mut CommandBuffer")
    }
}

impl SystemAccess for WithCmdMut {
    fn access(&self, _: &World, dst: &mut Vec<Access>) {
        dst.push(Access {
            kind: AccessKind::CommandBuffer,
            mutable: true,
        });
    }
}

/// Access schedule input
pub struct WithInput<T>(pub(crate) PhantomData<T>);

impl<'a, T: 'static> SystemData<'a> for WithInput<T> {
    type Value = AtomicRef<'a, T>;

    fn acquire(&'a mut self, ctx: &'a SystemContext<'_, '_, '_>) -> Self::Value {
        match ctx.input() {
            Some(v) => v,
            None => panic!("Input does not contain {}", tynm::type_name::<T>()),
        }
    }

    fn describe(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str("&")?;
        f.write_str(&tynm::type_name::<T>())
    }
}

impl<T: 'static> SystemAccess for WithInput<T> {
    fn access(&self, _: &World, dst: &mut Vec<Access>) {
        dst.push(Access {
            kind: AccessKind::Input(TypeId::of::<T>()),
            mutable: false,
        });
    }
}

/// Access schedule input mutably
pub struct WithInputMut<T>(pub(crate) PhantomData<T>);

impl<'a, T: 'static> SystemData<'a> for WithInputMut<T> {
    type Value = AtomicRefMut<'a, T>;

    fn acquire(&'a mut self, ctx: &'a SystemContext<'_, '_, '_>) -> Self::Value {
        match ctx.input_mut() {
            Some(v) => v,
            None => panic!("input does not contain `{}`", tynm::type_name::<T>()),
        }
    }

    fn describe(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str("&mut ")?;
        f.write_str(&tynm::type_name::<T>())
    }
}

impl<T: 'static> SystemAccess for WithInputMut<T> {
    fn access(&self, _: &World, dst: &mut Vec<Access>) {
        dst.push(Access {
            kind: AccessKind::Input(TypeId::of::<T>()),
            mutable: true,
        });
    }
}

#[cfg(test)]
mod test {
    use alloc::string::String;
    use atomic_refcell::AtomicRefMut;
    use itertools::Itertools;

    use crate::{
        component, components::name, filter::All, query::QueryData, system::SystemContext,
        CommandBuffer, Component, Entity, Query, QueryBorrow, World,
    };

    use super::{SystemData, SystemFn, WithWorldMut};

    component! {
        health: f32,
    }

    #[test]
    fn system_fn() -> anyhow::Result<()> {
        let mut world = World::new();
        let mut cmd = CommandBuffer::new();
        #[allow(clippy::let_unit_value)]
        let data = ();
        let ctx = SystemContext::new(&mut world, &mut cmd, &data);

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

        let data = &mut (WithWorldMut,);
        let data: (AtomicRefMut<World>,) = data.acquire(&ctx);
        SystemFn::<(AtomicRefMut<World>,), ()>::execute(&mut spawner, data);
        // (spawner).execute(data);

        let data = &mut (Query::new(name()),);
        let data = data.acquire(&ctx);
        SystemFn::<(QueryData<_>,), ()>::execute(&mut reader, data);
        Ok(())
    }
}
