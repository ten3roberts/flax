#[macro_export]
/// Declarative component generation
///
/// # Usage
/// ```rust
/// flax::component! {
///     health: f32,
/// }
/// ```
///
/// ```rust,ignore
/// flax::component! {
///     // component
///     pub name: type, // component
///
///     // component with metadata/reflection
///     pub(crate) name: type => [ Metadata, ... ],
///
///     // relational component
///     name(target): type
///
///     // relation component with metadata/reflection
///     name(target): type => [ Metadata, ... ]
///
///     // static entity
///     name,
/// }
/// ```
/// # Visibility
///
/// Components are by default only visible to the module they were declared in. However, any
/// visibility qualifier can be added before the name to expose it.
///
///
/// # Metadata
///
/// Metadata can be attached to any component, which allows reflection and
/// additional desc for components. Any type which implements [`crate::metadata::Metadata`] can be used.
///
/// The following allows the component value to be printed for the world debug
/// formatter, and it thus recommended to add where possible.
///
/// ```rust
/// use flax::component;
/// component! {
///     health: f32 => [flax::Debuggable],
///     position: (f32, f32) => [flax::Debuggable],
/// }
/// ```
///
/// # Relations
/// A component can be associated to another entity, which declares a relation of the component
/// type between the subject (entity which has the component), and the target (the associated
/// entity).
///
/// Relation components with different associated entities are distinct.
///
/// This is allows non random access hierachies, see: [guide:relations]( https://ten3roberts.github.io/flax/guide/fundamentals/relations.html )
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
///     connection(id): Joint => [flax::Debuggable],
/// }
/// ```
/// # Static Entity
///
/// Contrary to what the name may suggest, the macro can be used for static entity ids.
///
/// This may allow for crate-specific/non-global *resource* entities
/// Since a component is also an entity id, a raw static entity can also be
/// generated. This may allow for some *resource* entity or alike.
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
/// distinct with across different target.
macro_rules! component {
    // Relations
    ($(#[$outer:meta])* $vis: vis $name: ident( $obj: ident ): $ty: ty $(=> [$($metadata: ty),*])?, $($rest:tt)*) => {
        #[allow(dead_code)]
        $(#[$outer])*
        $vis fn $name($obj: $crate::Entity) -> $crate::Component<$ty> {

            static COMPONENT_ID: ::core::sync::atomic::AtomicU32 = ::core::sync::atomic::AtomicU32::new($crate::entity::EntityIndex::MAX);
            fn meta(_desc: $crate::component::ComponentDesc) -> $crate::buffer::ComponentBuffer {
                let mut _buffer = $crate::buffer::ComponentBuffer::new();

                <$crate::metadata::Name as $crate::metadata::Metadata<$ty>>::attach(_desc, &mut _buffer);
                <$crate::Component<$ty> as $crate::metadata::Metadata<$ty>>::attach(_desc, &mut _buffer);

                $(
                    $(
                        <$metadata as $crate::metadata::Metadata::<$ty>>::attach(_desc, &mut _buffer);
                    )*
                )*

                _buffer
            }

            static META: $crate::vtable::LazyComponentBuffer = $crate::vtable::LazyComponentBuffer::new(meta);
            static VTABLE: &$crate::vtable::ComponentVTable<$ty> =
                &$crate::vtable::ComponentVTable::new(stringify!($name), &META);
            use $crate::entity::EntityKind;
            use $crate::relation::RelationExt;
            $crate::Component::static_init(&COMPONENT_ID, EntityKind::COMPONENT, VTABLE).of($obj)
        }

        $crate::component!{ $($rest)* }
    };

    // Component
    ($(#[$outer:meta])* $vis: vis $name: ident: $ty: ty $(=> [$($metadata: ty),*])?, $($rest:tt)*) => {


        $(#[$outer])*
        $vis fn $name() -> $crate::Component<$ty> {
        static COMPONENT_ID: ::core::sync::atomic::AtomicU32 = ::core::sync::atomic::AtomicU32::new($crate::entity::EntityIndex::MAX);
            fn meta(_desc: $crate::component::ComponentDesc) -> $crate::buffer::ComponentBuffer {
                let mut _buffer = $crate::buffer::ComponentBuffer::new();

                <$crate::metadata::Name as $crate::metadata::Metadata<$ty>>::attach(_desc, &mut _buffer);
                <$crate::Component<$ty> as $crate::metadata::Metadata<$ty>>::attach(_desc, &mut _buffer);

                $(
                    $(
                        <$metadata as $crate::metadata::Metadata::<$ty>>::attach(_desc, &mut _buffer);
                    )*
                )*

                _buffer
            }

            static META: $crate::vtable::LazyComponentBuffer = $crate::vtable::LazyComponentBuffer::new(meta);
            static VTABLE: &$crate::vtable::ComponentVTable<$ty> =
                &$crate::vtable::ComponentVTable::new(stringify!($name), &META);
            use $crate::entity::EntityKind;
            $crate::Component::static_init(&COMPONENT_ID, EntityKind::COMPONENT, VTABLE)
        }

        $crate::component!{ $($rest)* }
    };

    // Entity
    ($(#[$outer:meta])* $vis: vis $name: ident, $($rest:tt)*) => {
        $(#[$outer])*
        $vis fn $name() -> $crate::Entity {
        static ENTITY_ID: ::core::sync::atomic::AtomicU32 = ::core::sync::atomic::AtomicU32::new($crate::entity::EntityIndex::MAX);
            $crate::Entity::static_init(&ENTITY_ID, $crate::entity::EntityKind::empty())
        }

        $crate::component!{ $($rest)* }
    };

    () => {}
}

#[macro_export]
/// Helper macro for creating a vtable for custom components
macro_rules! component_vtable {
    ($name:tt: $ty: ty $(=> [$($metadata: ty),*])?) => {

        {
            fn meta(_desc: $crate::component::ComponentDesc) -> $crate::buffer::ComponentBuffer {
                let mut _buffer = $crate::buffer::ComponentBuffer::new();

                <$crate::metadata::Name as $crate::metadata::Metadata<$ty>>::attach(_desc, &mut _buffer);
                <$crate::Component<$ty> as $crate::metadata::Metadata<$ty>>::attach(_desc, &mut _buffer);

                $(
                    $(
                        <$metadata as $crate::metadata::Metadata::<$ty>>::attach(_desc, &mut _buffer);
                    )*
                )*

                _buffer

            }

            static META: $crate::vtable::LazyComponentBuffer = $crate::vtable::LazyComponentBuffer::new(meta);
            static VTABLE: &$crate::vtable::ComponentVTable<$ty> =
                &$crate::vtable::ComponentVTable::new(stringify!($name), &META);

            VTABLE
        }

    };
}
