#[macro_export]
/// Generate a new component
///
/// # Usage
/// ```rust
/// use flax::component;
/// component! {
///     health: f32,
/// }
/// ```
///
/// Metadata can be attached to any component, which allows reflection and
/// additional info for components. Any type which implements [`crate::MetaData`] can be used.
///
/// The following allows the component value to be printed for the world debug
/// formatter, and it thus recommended to add where possible.
///
/// ```rust
/// use flax::component;
/// component! {
///     health: f32 => [flax::Debug],
///     position: (f32, f32) => [flax::Debug],
/// }
/// ```
///
/// A component can a relation, which allows a *normal* entity to be associated
/// inside the component id. Two relations with different *objects* are distinct
/// components. This is useful for hierarchies, see: [guide:hierarchy]( https://ten3roberts.github.io/flax/guide/fundamentals/hierarchy.html )
///
/// ```rust
/// use flax::component;
///
/// #[derive(Debug, Clone)]
/// struct Joint {
///     offset: f32,
///     strength: f32,
/// }
///
/// component! {
///     connection(id): Joint => [flax::Debug],
/// }
/// ```
///
/// Since a component is also an entity id, a raw static entity can also be
/// generated. This may allow for some *resource* entity or alike.
///
/// This is done by not giving a type to the component.
///
///
/// ```rust
/// use flax::component;
///
/// component! {
///     resource_entity,
/// }
/// ```
///
/// # Explanation
/// A component is nothing more but a mere typesafe entity id.
///
/// This macro uses an atomic to generate a lazily acquired
/// unique entity id through the [`crate::entity::EntityKind::STATIC`] bitflag. This flag
/// signifies to the world that the id essentially has a `'static` lifetime and
/// shall be treated as always existing, this allows one or more world to work
/// independently of the static components, alleviating the need for an `init`
/// function for each new world.
///
/// Since a component is either static, or have a lifetime managed by the world,
/// the upper bits containing the generation can be discarded and used to store
/// another *generationless* entity id.
///
/// This allows for the parameterization of components with component ids being
/// distinct with across different objects.
macro_rules! component {
    // Relations
    ($(#[$outer:meta])* $vis: vis $name: ident( $obj: ident ): $ty: ty $(=> [$($metadata: ty),*])?, $($rest:tt)*) => {
        $crate::paste! {
            #[allow(dead_code)]
            static [<COMPONENT_ $name:snake:upper _ID>]: ::core::sync::atomic::AtomicU32 = ::core::sync::atomic::AtomicU32::new(0);
            $(#[$outer])*
            $vis fn $name($obj: $crate::Entity) -> $crate::Component<$ty> {
                fn meta(_component: $crate::ComponentInfo) -> $crate::buffer::ComponentBuffer {
                    let mut _buffer = $crate::buffer::ComponentBuffer::new();

                    <$crate::Name as $crate::MetaData<$ty>>::attach(_component, &mut _buffer);
                    <$crate::Component<$ty> as $crate::MetaData<$ty>>::attach(_component, &mut _buffer);

                    $(
                        $(
                            <$metadata as $crate::MetaData::<$ty>>::attach(_component, &mut _buffer);
                        )*
                    )*

                    _buffer
                }

                use $crate::entity::EntityKind;
                use $crate::RelationExt;
                $crate::Component::static_init(&[<COMPONENT_ $name:snake:upper _ID>], EntityKind::COMPONENT, stringify!($name), meta).of($obj)
            }
        }

        $crate::component!{ $($rest)* }
    };

    // Component
    ($(#[$outer:meta])* $vis: vis $name: ident: $ty: ty $(=> [$($metadata: ty),*])?, $($rest:tt)*) => {

        $crate::paste! {
            #[allow(dead_code)]
            static [<COMPONENT_ $name:snake:upper _ID>]: ::core::sync::atomic::AtomicU32 = ::core::sync::atomic::AtomicU32::new(0);
            $(#[$outer])*
            $vis fn $name() -> $crate::Component<$ty> {
                fn meta(_component: $crate::ComponentInfo) -> $crate::buffer::ComponentBuffer {
                    let mut _buffer = $crate::buffer::ComponentBuffer::new();

                    <$crate::Name as $crate::MetaData<$ty>>::attach(_component, &mut _buffer);
                    <$crate::Component<$ty> as $crate::MetaData<$ty>>::attach(_component, &mut _buffer);

                    $(
                        $(
                            <$metadata as $crate::MetaData::<$ty>>::attach(_component, &mut _buffer);
                        )*
                    )*

                    _buffer
                }
                use $crate::entity::EntityKind;
                $crate::Component::static_init(&[<COMPONENT_ $name:snake:upper _ID>], EntityKind::COMPONENT, stringify!($name), meta)
            }
        }

        $crate::component!{ $($rest)* }
    };

    // Entity
    ($(#[$outer:meta])* $vis: vis $name: ident, $($rest:tt)*) => {
        $crate::paste! {
            #[allow(dead_code)]
            static [<ENTITY_ $name:snake:upper _ID>]: ::core::sync::atomic::AtomicU32 = ::core::sync::atomic::AtomicU32::new(0);
            $(#[$outer])*
            $vis fn $name() -> $crate::Entity {
                $crate::Entity::static_init(&[<ENTITY_ $name:snake:upper _ID>], $crate::entity::EntityKind::empty())
            }
        }

        $crate::component!{ $($rest)* }
    };

    () => {}
}
