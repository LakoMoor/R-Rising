# Candidate gameplay player animation inventory

On the pinned client installation, run `VRISING_DATA=... cargo run -p rrising-content
--example player_animation_inventory`. The candidate male gameplay prefab's Animator is
attached to `VampireMale_Standard_LOD00_rig`. Its generic (non-humanoid) Avatar has 132
skeleton nodes. The Animator uses an `AnimatorOverrideController` whose base controller
lists 1,891 clip references (including duplicates). The diagnostic reads the base controller;
it does not apply overrides or reconstruct runtime state selection.

The body and cape refer to 85 distinct bone paths below this Animator. In the base
controller's `VampireMale_Unarmed_Walk_Forward`, the decoder reports 394 tracks; 255
Transform tracks hash to those bone paths. The `VampireMale_Unarmed_Run_Forward` entry
likewise has 394 tracks and 255 matching Transform tracks. These counts are track matches,
not distinct bones or proof of animation parity. Path-hash collisions have not been excluded.
Evaluating curves, applying the hierarchy and binding an animation state to actual gameplay
were not covered by that inventory. The experimental `player_clip_sample` example now samples
translation, quaternion and scale tracks and composes the bone hierarchy. On the pinned client,
the sampled rest hierarchy (with tracks removed) matches scene matrices within 1e-8, and all
80 body bones differ between 0 and 0.3 seconds of the first walk clip. This is a self-consistency
check, not a comparison against Unity. Controller overrides, blending, transitions, Euler tracks,
root motion and gameplay state selection remain unimplemented.

The `character_pose` diagnostic accepts a fourth argument with a sample time. On the pinned
male gameplay asset the first base-controller clip (`VampireMale_Unarmed_Walk_Forward`) was
sampled at 0 and 0.3 seconds and skinned to the body and cape using a fixed camera. The
[first frame](../docs/screenshots/experimental-walk-0.png) and
[second frame](../docs/screenshots/experimental-walk-03.png) visibly differ. These frames
have neutral materials, no head and no gameplay state; they have not been compared to Unity.
