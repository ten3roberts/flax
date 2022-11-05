# Changelog

All notable changes to this project will be documented in this file.

## [0.3.1] - 2022-11-05

### Bug Fixes

- Set(_with) not working for reserved entities
- Make EntityIndex primitive
- No-default-features lints
- Blanklines in example
- Doclinks in README

### Features

- Filter subscription
- Tokio subscribers
- Extensible event subscription

### Miscellaneous Tasks

- CHANGELOG.md
- Fix tests
- Simplify internal archetype borrowing api
- Fix no-std
- Fix warnings
- Remove duplicate simpler event_registry
- Doclinks

### Refactor

- Archetype change events

### Testing

- Change subscribing
- Subscribe
- Sparse or combinators

## [0.3.0] - 2022-10-18

### Bug Fixes

- Ron ident deserialize
- Rename serde module due to crate:serde collision
- Change list remove performance
- Schedule granularity
- Unnecessary checks
- Feature gated benchmarks
- Doctests
- Warnings
- Badge links
- Quasi-quadratic growth strategy
- Whitespace in badges
- Warnings
- No_std tests
- Auto spawn static entities
- Cmds not applied in schedule_seq
- Artefact location
- Dead links
- Feature gate flume due to std requirement
- Asteroids deps
- Spacing
- Use describe rather than requiring debug for filters

### Features

- Benchmarking
- Batch_size
- Human friendly access info
- Query trie archetype searching
- Row and column serialize benchmarks
- Par_for_each
- No_std
- Rework components and relations
- Concurrently reserve entities
- Asteroids wasm example
- EntityQuery
- Make Query::get use filters
- Require `Filter` to implement bitops
- Make merge_with append to static ids (instead of ignoring and dropping components)

### Miscellaneous Tasks

- Add guide badge
- Add keywords
- Inline some hot callsites
- Remove tynm
- Fix unused imports with --no-default-features
- Merge deployment of guide and asteroids demo
- Change guide location
- Consistent workflow names
- Use EntityQuery in asteroids
- Remove unneded `fetch::missing`
- [**breaking**] Rename `is_component` => `component_info`
- Cleanup docs
- Make rayon examples use custom thread pool
- Fix doctests

### Refactor

- Use a freelist vec instead of inplace linked list

### Testing

- System access and scheduling
- Filter combinators

### Wip

- Improve remove performance
- Component relation rework

## [0.2.0] - 2022-09-11

### Bug Fixes

- PreparedQuery re-entrancy
- Wip issues
- Spawn_at
- Empty entities in root archetype
- Guide workflow
- Guide workflow
- Assertion not respecting groups
- Non sorted change list
- Release assertion on non unqiue fn instances
- Id recycling
- Update markdown title
- Docs and unnused items
- Dead code
- ComponentBuffer non deterministic iteration order
- Clippy lints
- Cursor position outside buffer
- Vastly simplify system traits
- Docs and warnings
- Don't expose rexport buffer
- Inconsistent Fetch trait
- Bincode serialization
- On_remove not triggered for clear
- Merge with holes in entity ids
- Commandbuffer events not happening in order
- Query not recalculating archetypes when entity moves to existing but empty arch
- Change event wrapping
- Warnings
- SystemFn describe
- Use of unstable features
- Imports and serde api
- QueryBorrow::get
- Broken link
- Miri
- Badge style
- Make queries skip empty archetypes in access
- Sync readme
- Execute schedule in doc test
- Test with all features
- Wrapped line in docs
- Hide extraneous bracket
- Docs
- Stable archetype gen
- Unused deps
- Public api
- Cleanup public api
- Continue api cleanup
- Link style
- Missing import
- Broken doclinks
- Derive docs
- Manifest
- Bump deps
- Eprintln

### Documentation

- Relations

### Features

- Change around world access
- Parallel scheduling
- Optional queries
- Entity ref
- Entry like component and entity api
- Standard components
- Component metadata and components
- Implement debug for world
- Batched iteration
- With_world and with_cmd
- Detach relation when subject is despawned
- Tracing
- Clear entity
- EntityBuilder hierarchy
- User guide
- Query
- Schedule
- Filter for &Filter
- Relation and wildcard for `with` and `without`
- Make storage self contained
- Batch insert
- Column serialization and deserialization
- Row and column serialization
- Relations_like
- Entity builder and batch spawn
- Cmd batch
- Hierarchy
- Commandbuffer
- FetchItem
- Allow filters to be attached directly to a fetch
- Merge worlds
- Merge custom components and relations
- Fast path on extend for empty archetype
- On_removed channel
- Shared system resource
- Use normal references in systems
- Allow schedle introspection
- Merge change ticks
- Auto opt in test
- Feature gate implementation detail asserts
- Serialization

### Miscellaneous Tasks

- Remove dbg prints
- Fix all warnings
- Apply clippy lints
- Add guide to readme
- More comments in examples
- Sync readme
- More links
- Small changes
- Reduce items in prelude
- Change default query generics
- Custom EntityKind [de]serialize implementation
- Sync readme
- Link relations in docs
- Sync readme
- Bump version

### Refactor

- Simplify filter
- Archetype storage
- Entity spawning
- Change list
- Shared resource

### Update

- Workflows

### Wip

- Parallel schedule
- Opt
- Component metadata
- Component metadata
- Component detaching
- Query
- Schedule
- Batch insert
- Reserve
- Serialize
- Query advanced
- Relations
- Relations
- Derive fetch
- Derive
- Partial Fetch proc macro impl
- Asteroids examples

<!-- generated by git-cliff -->
