# Serialization and deserialization

While the built in reflection system could be used for serialization, similar
to `Debug`, deserialization can not. This is because the lazy registration of
components mean that the world may not know of the componenets deserialization
upfront, especially since deserialization yields a new world.

In addition, having serialization be _implicit_ may lead to components not being
serialized when they are expected to, or components which should not be
serialized to be written to disk, such as local state. As such, it leads to
unexpected, undesirable, or downright insecure behavior.

A similar story is to be found for _deserialization_, where program behaviour
can be intrusively modified due to anything being able to be deserialized and
put into the world.

As such \[de\]serialization is explicit and requires registering a subset of
components.
