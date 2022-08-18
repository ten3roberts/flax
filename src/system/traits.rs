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
    fn access(&mut self, world: &World) -> Vec<Access>;
}

/// Trait for any function `Fn(Args) -> Ret)` or similar which is callable with
/// the provided context
pub trait SystemFn<'a, Ctx, Args, Ret> {
    /// Executes a system with the associated data
    fn execute(&'a mut self, ctx: Ctx) -> Ret;
    /// Human friendly description of this system
    fn describe(&self, f: &mut Formatter<'_>) -> fmt::Result;
    /// Returns the data accesses of a system function
    fn access(&'a mut self, ctx: Ctx) -> Vec<Access>;
}

struct Verbatim(String);
impl fmt::Debug for Verbatim {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

macro_rules! tuple_impl {
    ($($idx: tt => $ty: ident),*) => {
        // Fallible
        impl<'a, Func, Ret, $($ty,)* > SystemFn<'a, (&'a SystemContext<'a>, &'a mut ($($ty,)*)), ($($ty::Data,)*), Ret> for Func
        where
            Func: FnMut($($ty::Data,)*) -> Ret,
            $($ty: SystemData<'a> + SystemAccess,)*
        {
            fn execute(&mut self, (ctx, data): (&'a SystemContext<'a>, &'a mut ($($ty,)*))) -> Ret {
                let _data = data.get(ctx).expect("Failed to get system data");
                (self)($((_data.$idx),)*)
            }

            fn describe(&self, f: &mut Formatter<'_>) -> fmt::Result {
                use std::fmt::Debug;
                f.write_str("Fn")?;

                ($(
                        Verbatim(tynm::type_name::<$ty>()),
                )*).fmt(f)?;

                if std::any::type_name::<Ret>() != std::any::type_name::<()>() {
                    write!(f, " -> {}", tynm::type_name::<Ret>())?;
                }

                Ok(())
            }

            fn access(&'a mut self, (ctx, data): (&'a SystemContext<'a>, &'a mut ($($ty,)*))) -> Vec<Access> {
                let world = ctx.world().unwrap();
                data.access(&world)
            }
        }

        impl<$($ty,)*> SystemAccess for ($($ty,)*)
        where
            $($ty: SystemAccess,)*
        {
            fn access(&mut self, world: &World) -> Vec<Access> {
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

            fn get(&'w mut self, _ctx: &'w SystemContext<'w>) -> eyre::Result<Self::Data> {
                Ok(
                    ($((self.$idx).get(_ctx)?,)*)
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
    fn get(&'a mut self, ctx: &'a SystemContext<'a>) -> eyre::Result<Self::Data>;
}

/// Access part of the context mutably.
#[doc(hidden)]
pub struct Writable<T>(PhantomData<T>);
#[doc(hidden)]
pub struct Readable<T>(PhantomData<T>);
#[derive(Debug)]
/// Allows mutable access to data in the system context
pub struct Write<'a, T>(AtomicRefMut<'a, &'a mut T>);

#[derive(Debug)]
/// Allows immutable access to data in the system context
pub struct Read<'a, T>(AtomicRef<'a, &'a mut T>);

impl<'a, T> Deref for Read<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl<'a, T> Deref for Write<'a, T> {
    type Target = &'a mut T;

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

    fn get(&mut self, ctx: &'a SystemContext<'a>) -> eyre::Result<Self::Data> {
        Ok(Write(
            ctx.world_mut()
                .map_err(|_| eyre!("Failed to borrow world mutably"))?,
        ))
    }
}

impl<'a> SystemData<'a> for Readable<World> {
    type Data = Read<'a, World>;

    fn get(&mut self, ctx: &'a SystemContext<'a>) -> eyre::Result<Self::Data> {
        Ok(Read(
            ctx.world()
                .map_err(|_| eyre!("Failed to borrow world mutably"))?,
        ))
    }
}

impl SystemAccess for Writable<World> {
    fn access(&mut self, _: &World) -> Vec<Access> {
        vec![Access {
            kind: AccessKind::World,
            mutable: true,
        }]
    }
}

impl SystemAccess for Readable<World> {
    fn access(&mut self, _: &World) -> Vec<Access> {
        vec![Access {
            kind: AccessKind::World,
            mutable: false,
        }]
    }
}

impl<'a> SystemData<'a> for Writable<CommandBuffer> {
    type Data = Write<'a, CommandBuffer>;

    fn get(&mut self, ctx: &'a SystemContext<'a>) -> eyre::Result<Self::Data> {
        Ok(Write(ctx.cmd_mut().map_err(|_| {
            eyre!("Failed to borrow commandbuffer mutably")
        })?))
    }
}

impl SystemAccess for Writable<CommandBuffer> {
    fn access(&mut self, _: &World) -> Vec<Access> {
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
        component, components::name, system::SystemContext, CommandBuffer, Entity, Query,
        QueryData, SystemFn, World,
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
                .spawn(*w);

            Entity::builder()
                .set(name(), "Trinity".to_string())
                .set(health(), 85.0)
                .spawn(*w);
        };

        let mut reader = |mut q: QueryData<_, _>| {
            let names = q.iter().iter().cloned().sorted().collect_vec();

            assert_eq!(names, ["Neo", "Trinity"]);
        };

        let data = &mut (Writable::<World>::new(),);

        (spawner).execute((&ctx, data));

        let data = &mut (Query::new(name()),);
        (reader).execute((&ctx, data));
        Ok(())
    }
}
