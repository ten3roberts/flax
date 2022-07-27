#[macro_export]
macro_rules! hash {
    ($s:expr) => {{
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let id = $s;

        let mut s = DefaultHasher::new();
        id.hash(&mut s);
        s.finish()
    }};
    () => {{
        let id = concat!(file!(), line!(), column!());
        hash!(id)
    }};
    ($($s:expr),*) => {{
        let mut s: u128 = 0;
        $(s += $crate::hash!($s) as u128;)*
        $crate::hash!(s)
    }};
}

#[macro_export]
/// Generate a new component
/// usage:
/// ```rust
/// use flax::component;
/// component! {
///     health: f32,
///     connection(id): f32,
/// }
/// ```
/// This will create a function `health()` which returns the component id.
macro_rules! component {
    // Relations
    ($(#[$outer:meta])* $vis: vis $name: ident( $obj: ident ): $ty: ty $(=> [$($metadata: ty),*])?, $($rest:tt)*) => {
        $crate::paste! {
            #[allow(dead_code)]
            static [<COMPONENT_ $name:snake:upper _ID>]: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
            $(#[$outer])*
            $vis fn $name($obj: $crate::Entity) -> $crate::Component<$ty> {
                fn meta(_component: $crate::ComponentInfo) -> $crate::ComponentBuffer {
                    let mut _buffer = $crate::ComponentBuffer::new();

                    <$crate::Name as $crate::MetaData<$ty>>::attach(_component, &mut _buffer);
                    <$crate::Component<$ty> as $crate::MetaData<$ty>>::attach(_component, &mut _buffer);

                    $(
                        $(
                            <$metadata as $crate::MetaData::<$ty>>::attach(_component, &mut _buffer);
                        )*
                    )*

                    _buffer
                }

                use $crate::EntityKind;
                $crate::Component::new($crate::Entity::static_init(&[<COMPONENT_ $name:snake:upper _ID>], EntityKind::COMPONENT | EntityKind::RELATION), stringify!($name), meta).into_pair($obj)
            }
        }

        $crate::component!{ $($rest)* }
    };

    // Component
    ($(#[$outer:meta])* $vis: vis $name: ident: $ty: ty $(=> [$($metadata: ty),*])?, $($rest:tt)*) => {

        $crate::paste! {
            #[allow(dead_code)]
            static [<COMPONENT_ $name:snake:upper _ID>]: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
            $(#[$outer])*
            $vis fn $name() -> $crate::Component<$ty> {
                fn meta(_component: $crate::ComponentInfo) -> $crate::ComponentBuffer {
                    let mut _buffer = $crate::ComponentBuffer::new();

                    <$crate::Name as $crate::MetaData<$ty>>::attach(_component, &mut _buffer);
                    <$crate::Component<$ty> as $crate::MetaData<$ty>>::attach(_component, &mut _buffer);

                    $(
                        $(
                            <$metadata as $crate::MetaData::<$ty>>::attach(_component, &mut _buffer);
                        )*
                    )*

                    _buffer
                }
                use $crate::EntityKind;
                $crate::Component::new($crate::Entity::static_init(&[<COMPONENT_ $name:snake:upper _ID>], EntityKind::COMPONENT), stringify!($name), meta)
            }
        }

        $crate::component!{ $($rest)* }
    };

    // Entity
    ($(#[$outer:meta])* $vis: vis $name: ident, $($rest:tt)*) => {
        $crate::paste! {
            #[allow(dead_code)]
            static [<ENTITY_ $name:snake:upper _ID>]: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
            $(#[$outer])*
            $vis fn $name() -> $crate::Entity {
                $crate::Entity::static_init(&[<ENTITY_ $name:snake:upper _ID>], EntityKind::empty())
            }
        }

        $crate::component!{ $($rest)* }
    };

    () => {}
}
