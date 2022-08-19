use std::{
    fmt::{self, Formatter},
    marker::PhantomData,
    ops::{Deref, DerefMut},
};

use atomic_refcell::{AtomicRef, AtomicRefMut};
use eyre::eyre;

use crate::{AccessKind, CommandBuffer, World};

use super::{cell::SystemContext, Access};

/// Describe an access to the world in ters of shared and unique accesses
pub trait SystemAccess {
    /// Returns all the accesses for a system
    fn access(&self, world: &World) -> Vec<Access>;
}

/// A callable function
pub trait Callable<'this, Args, Ret> {
    /// exe
    fn execute(&'this mut self, args: Args) -> Ret;
    /// Debug for Fn
    fn describe(&self, f: &mut Formatter<'_>) -> fmt::Result;
    /// Returns the data accesses of a system function
    fn access(&self, world: &World) -> Vec<Access>;
}

struct Verbatim(String);
impl fmt::Debug for Verbatim {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

macro_rules! tuple_impl {
    ($($idx: tt => $ty: ident),*) => {
        impl<'this, Func, Ret, $($ty,)*> Callable<'this, ($($ty,)*), Ret> for Func
        where Func: FnMut($($ty,)*) -> Ret
        {
            fn execute(&'this mut self, _args: ($($ty,)*)) -> Ret {
                (self)($(_args.$idx,)*)
            }

            fn describe(&self, f: &mut Formatter<'_>) -> fmt::Result {
                use std::fmt::Debug;
                f.write_str("fn")?;

                ($(
                    Verbatim(tynm::type_name::<$ty>()),
                )*).fmt(f)?;

                if std::any::type_name::<Ret>() != std::any::type_name::<()>() {
                    write!(f, " -> {}", tynm::type_name::<Ret>())?;
                }

                Ok(())
            }

            fn access(&self, _: &World) -> Vec<Access> {
                vec![]
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

        impl<'w, $($ty,)*> SystemData<'w> for ($($ty,)*)
        where
            $($ty: SystemData<'w>,)*
        {
            type Data = ($(<$ty as SystemData<'w>>::Data,)*);

            fn bind(&'w mut self, _ctx: &'w SystemContext<'_>) -> eyre::Result<Self::Data> {
                Ok(
                    ($((self.$idx).bind(_ctx)?,)*)
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

/// Describes a type which can fetch assocated Data from the system context and
/// provide it to the system.
pub trait SystemData<'a>: SystemAccess {
    /// The borrow from the system context
    type Data;
    /// Get the data from the system context
    fn bind(&'a mut self, ctx: &'a SystemContext<'_>) -> eyre::Result<Self::Data>;
}

/// Access part of the context mutably.
#[doc(hidden)]
pub struct Writable<T>(PhantomData<T>);
#[doc(hidden)]
pub struct Readable<T>(PhantomData<T>);
#[derive(Debug)]
/// Allows mutable access to data in the system context
pub struct Write<'a, T>(AtomicRefMut<'a, T>);

#[derive(Debug)]
/// Allows immutable access to data in the system context
pub struct Read<'a, T>(AtomicRef<'a, T>);

impl<'a, T> Deref for Read<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl<'a, T> Deref for Write<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl<'a, T> DerefMut for Write<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.deref_mut()
    }
}

impl<T> Writable<T> {
    pub(crate) fn new() -> Self {
        Self(PhantomData)
    }
}

impl<T> Readable<T> {
    pub(crate) fn new() -> Self {
        Self(PhantomData)
    }
}

impl<T> Default for Readable<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Default for Writable<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> SystemData<'a> for Writable<World> {
    type Data = Write<'a, World>;

    fn bind(&mut self, ctx: &'a SystemContext<'_>) -> eyre::Result<Self::Data> {
        Ok(Write(
            ctx.world_mut()
                .map_err(|_| eyre!("Failed to borrow world mutably"))?,
        ))
    }
}

impl<'a> SystemData<'a> for Readable<World> {
    type Data = Read<'a, World>;

    fn bind(&mut self, ctx: &'a SystemContext<'_>) -> eyre::Result<Self::Data> {
        Ok(Read(
            ctx.world()
                .map_err(|_| eyre!("Failed to borrow world mutably"))?,
        ))
    }
}

impl SystemAccess for Writable<World> {
    fn access(&self, _: &World) -> Vec<Access> {
        vec![Access {
            kind: AccessKind::World,
            mutable: true,
        }]
    }
}

impl SystemAccess for Readable<World> {
    fn access(&self, _: &World) -> Vec<Access> {
        vec![Access {
            kind: AccessKind::World,
            mutable: false,
        }]
    }
}

impl<'a> SystemData<'a> for Writable<CommandBuffer> {
    type Data = Write<'a, CommandBuffer>;

    fn bind(&mut self, ctx: &'a SystemContext<'_>) -> eyre::Result<Self::Data> {
        Ok(Write(ctx.cmd_mut().map_err(|_| {
            eyre!("Failed to borrow commandbuffer mutably")
        })?))
    }
}

impl SystemAccess for Writable<CommandBuffer> {
    fn access(&self, _: &World) -> Vec<Access> {
        vec![Access {
            kind: AccessKind::CommandBuffer,
            mutable: true,
        }]
    }
}

#[cfg(test)]
mod test {

    use itertools::Itertools;

    use crate::{
        component, components::name, system::SystemContext, Callable, CommandBuffer, Entity, Query,
        QueryData, SystemData, World,
    };

    use super::{Writable, Write};

    component! {
        health: f32,
    }

    #[test]
    fn system_fn() -> eyre::Result<()> {
        let mut world = World::new();
        let mut cmd = CommandBuffer::new();
        let ctx = SystemContext::new(&mut world, &mut cmd);

        let mut spawner = |mut w: Write<_>| {
            Entity::builder()
                .set(name(), "Neo".to_string())
                .set(health(), 90.0)
                .spawn(&mut w);

            Entity::builder()
                .set(name(), "Trinity".to_string())
                .set(health(), 85.0)
                .spawn(&mut w);
        };

        let mut reader = |mut q: QueryData<_, _>| {
            let names = q.iter().iter().cloned().sorted().collect_vec();

            assert_eq!(names, ["Neo", "Trinity"]);
        };

        let data = &mut (Writable::<World>::new(),);

        (spawner).execute(data.bind(&ctx).unwrap());

        let data = &mut (Query::new(name()),);
        (reader).execute(data.bind(&ctx).unwrap());
        Ok(())
    }
}
