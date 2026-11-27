# Candidate native action-bar frame

On pinned client `a23f36d5…`, serialized file `26fae5e0b4c6a9c2db14710268c2d097`
contains an `ActionBar` GameObject and eight direct `ActionBarEntry` children. Each child's
`Background/Frame` has a `UnityEngine.UI.Image` whose build-specific serialized pointer at
byte 56 resolves to the same `ItemFrame_Normal` Sprite. Its unpacked texture rectangle covers
the complete 132×132 texture. The selected RectTransforms give a 48×48 slot, frame size delta
6×6 and action-bar anchored position (-103, 80); the horizontal-layout payload gives spacing 8.
These are stored prefab observations, not a runtime screenshot or proof of layout behavior.

`rrising-content::hud::candidate_action_bar` checks the eight entries, sprite identity, texture
shape, dimensions and bounded payloads. `reads_action_bar_frame_from_original_prefab` passed on
the owner's installation. `--diagnostic-hud` uploads the texture and draws eight frames via a
new alpha-blended GPU overlay; a synthetic offscreen test checks its position and transparency.
The resulting [Windows frame](../docs/screenshots/action-bar-diagnostic.png) was rendered with
the actual scene; the native window was also visually inspected.

The parent Canvas, layout group and CanvasScaler are not evaluated. The diagnostic assumes a
center-bottom parent and a 1080-pixel reference height, and draws no item icons, numbers, health,
minimap or input response. It is not an original HUD or gameplay comparison. Browser overlay
integration and a controlled original-game reference scenario remain outstanding.
