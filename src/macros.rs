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
///     health: f32
/// }
/// ```
/// This will create a function `health()` which returns the component id.
macro_rules! component {
    ($name: ident: $ty: ty) => {

        $crate::paste! {
            #[allow(dead_code)]
            static [<COMPONENT_ $name:snake:upper _ID>]: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
            pub fn $name() -> $crate::Component<$ty> {
                $crate::Component::static_init(&[<COMPONENT_ $name:snake:upper _ID>], stringify!($name))
            }
        }
    };
    ($($name: ident: $ty: ty,)*) => {
        $(
        $crate::component!{ $name: $ty }
    ) *
    }
}
