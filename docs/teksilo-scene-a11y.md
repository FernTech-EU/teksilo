<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Scene accessibility

A scene's visual arrangement is not necessarily a useful reading order. Use the
scene accessibility API to name meaningful items, group them logically, and
control off-screen enumeration.

## Minimal example

For a small scene whose items should remain enumerable outside the viewport:

```rust
use teksilo_scene::{A11yOffScreenMode, Scene, SceneView};

fn main() {
    let scene = Scene::new();
    let _view = SceneView::new(scene)
        .a11y_off_screen_mode(A11yOffScreenMode::AllItems);
}
```

Populate the scene as shown in the [scene guide](teksilo-scene.md). Test this
policy with representative data before applying it to a large scene.

## Common operations

| Task | API or policy |
| --- | --- |
| Limit off-screen enumeration | `A11yOffScreenMode` |
| Use visual structure as the default | `A11yMode::Cooperative` |
| Supply a separate logical structure | `A11yMode::StrictlyParallel` |
| Group related items | `A11yGroup` and logical parent relationships |
| Expose relationships | Scene accessibility relation APIs |
| Announce relevant changes | Live-region settings on logical nodes |
| Label or adjust a widget | [Accessibility overrides](accessibility-overrides.md) |

`ViewportPlusN { n: 1 }` is the default off-screen policy. `ViewportOnly` limits
enumeration to the viewport. `AllItems` keeps the complete scene available and
can increase the number of live widgets and accessibility nodes.

## Visibility and focus

Retention controls which heavyweight widgets stay alive. Accessibility policy
controls which items are enumerated. A retained widget outside the accessibility
region can remain alive without appearing in the accessibility tree.

Focus, pointer capture, and active drags can keep a card live outside the usual
retention region. Check the resulting keyboard order and screen-reader behavior
when changing retention or enumeration settings.

## Constraints

- Decorative geometry should not create unexplained focus targets.
- Avoid announcing every pointer sample or every paint update. Announce semantic
  changes that help the user understand the result.
- Provide keyboard operations for actions exposed through dragging or magnets.
- Logical groups and relations need stable targets after removal or replacement.
- Framework nodes and actions do not establish application accessibility. Test
  representative scenes with keyboard navigation and target screen readers.

## Reference

- [Scene accessibility source](../crates/teksilo-scene/src/a11y.rs)
- [Scene API](scene/index.md)
- [Corkboard example](../examples/scene_corkboard/src/main.rs)


## Engineering reference

[Implementation details and review history](https://github.com/ferntech-eu/teksilo/blob/main/engineering/docs/teksilo-scene-a11y.md)
are retained in the repository.
