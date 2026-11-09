# UnrealScript: lighting

[Agent entry point](../AGENTS.md). Read only the sections relevant to the task.

Paths in this reference retain their original project/repository context; verify details against
current sources.

### Static lighting (`lightbake/`)

`upk light info|export|stats|bake`; Studio `light.info` / `light.bake`. What the game reads (all of
it stock UE3 10897: `UnLightMap.cpp`, `LightMapRendering.h`, `BasePassPixelShader.usf`, Lightmass
`LightingSystem.cpp` / `LightmapData.cpp` / `TextureMapping.cpp` / `Scene.cpp`; the
three-coefficient layout is the source's `NUM_STORED_LIGHTMAP_COEF`, not a Hi-Rez change):

- A static mesh component's `LODData[lod].LightMap` is an `FLightMap2D`: `LightGuids` (the baked
  lights' **`LightmapGuid`**, not `LightGuid`: on Abyss every listed GUID matches a light's
  `LightmapGuid`), then per coefficient a texture and a scale (`FVector`), `CoordinateScale`,
  `CoordinateBias`. Coefficient 0 is `NormalizedAverageColor<i>_<n>`, 1
  `DirectionalMaxComponent<i>_<n>`, 2 (simple) is `None` with scale 0 in every shipped component
  (491 on Abyss) because the cooker's maps were built without simple light maps. The renderer treats
  a static light as baked for a component when its `LightmapGuid` is in the list
  (`FStaticMeshSceneProxy::FLODInfo::GetInteraction`), else as dynamic unless listed in
  `IrrelevantLights` (no shipped component stores that property); 8.1 ships almost no shadow maps
  (122 `ShadowMap2D` in the whole client) and no dominant lights.
- Encoding (`encode.rs`): `ConvertToLightSample` + `QuantizeLightSamples` (per component and
  channel, max starting at 1.0 and capped at 16, `trunc(pow(x/max, 1/2.2)·255)`); decode
  `(b/255)^2.2 · scale`; irradiance along the vertex normal `NAC · avg(DMC)`. Evidence (`upk light
  stats` over the 10 Abyss packages): every NAC scale is exactly 1.0, every DMC scale is in [1, 16]
  (the Lightmass quantization path; the editor's own path starts at 0); of 11.39 M bright texels
  (DMC average byte > 64) 11.37 M (99.85 %) have a NAC maximum channel ≥ 247, i.e. the average
  colour is normalised by its largest channel; 488 of 491 components' rectangles are
  `CoordinateScale·atlas` + 2 padding texels (`CoordinateBias = (x + 1) / atlas`: Lightmass
  `bPadMappings`, the mapping's inner `size - 2` square, edges copied outwards by
  `PadTextureMapping`). Atlases are 1024² DXT1 with streamed mips (`LightmapFlags` 1 =
  `LMF_Streamed` on all 226 textures). Re-encoding: decoding our own written textures back gives a
  mean |Δ| of 0.0028 against the floats (mean 0.0929 vs 0.0947 over Abyss_Art_Walls: DXT1 + the
  truncating quantizer).
- Baking (`bake.rs`, ported literally where Lightmass has it): texels `uv·N − 0.5` on the light map
  channel (`LightMapCoordinateIndex`, `N = resolution − 2`, resolution from
  `bOverrideLightMapRes`/`OverriddenLightMapRes` else the mesh's `LightMapResolution`), conservative
  mapping of grazed texels; point/spot radial `pow(max(1 − (d/R)², 0), FalloffExponent)`, spot cone
  squared, `FLinearColor(LightColor)·Brightness`, sky lights as `FGatheredLightSample::SkyLight`
  (shadowed sky by hemisphere rays), one-sided `pow(L.z, 0.85)·2` on the directional coefficients;
  shadow rays against every shadow-casting static mesh of the map (BVH), area lights over
  `LightSourceRadius` / `LightSourceAngle`; lights are relevant when their `LightingChannels`
  overlap the primitive's (the primitive's default is all off; `UPrimitiveComponent::Attach` sets
  `Static` when none is set; light default BSP/Static/Dynamic/CompositeDynamic) and
  `F*Light::AffectsBounds` reaches the component's bounds. With those rules a rebake of
  Abyss_Art_Walls lists exactly the shipped lights for 179 of 187 components. Light transforms:
  `LightToWorld` from the collection's stored parent matrix (`StaticLightCollectionActor`),
  direction = the parent's X axis, point/spot position `Translation · parent`. Dominant lights are
  skipped (they stay dynamic).
- Direct-only rebakes against the shipped Lightmass maps (`upk light bake … --compare`): one wall
  component (`staticmeshactor_smc_1`) mean irradiance 0.330 vs 0.341, median ratio 0.97, mean |Δ|
  0.036; all of Abyss_Art_Walls 0.095 vs 0.105, mean |Δ| 0.031, but the median per-texel ratio
  averages 0.51 (dark texels lack Lightmass' bounced light; TD_Abyss_P's world uses 5 bounces,
  `EnvironmentColor` (105,73,209)×2.25, `DiffuseBoost` 2.25 and ambient occlusion to 1600 units).
  The one-bounce approximation (`--indirect N`, albedo 0.15 × DiffuseBoost, environment radiance
  `EnvironmentColor/π`, ambient occlusion as in `IncomingRadianceUniform`) moves three walls from
  0.212 to 0.270 against 0.219 shipped (median ratio 0.76 → 1.44), and all of Abyss_Art_Walls to
  0.147 against 0.105 (median ratio 1.90, 207 s with 32 rays instead of 15 s): closer in the
  shadows, too bright in the open, since materials' real albedo is not evaluated. Direct-only stays
  the default.
- Writing (`write.rs`): allocations shelf-packed on 4-texel boundaries into power-of-two atlases ≤
  1024, two textures per atlas copied from a light map texture of the level (or another level of the
  map), DXT1 mips built like `FLightMapPendingTexture::StartEncoding` (coverage-weighted
  downsampling, the one-sided texel expansion), stored inline with `NeverStream` and `LightmapFlags`
  0; the component's LOD 0 gets the light map (every relevant light listed, lit or shadowed, so none
  renders dynamically on top), shadow maps cleared, lower LODs' light maps cleared (none on Abyss).
  Checked: TD_Abyss_P with Abyss_Art_Walls rebaked (187 components, 44 atlases) and a copied wall +
  a new StaticMeshActor from a mesh baked through `light.bake {scope: "edited"}`: `upk verify` 0
  mismatches, the textures decode back from the saved files. Not done: vertex light maps (components
  without light map UVs are skipped), BSP, terrain, the precomputed light volume (dynamic objects
  keep the original's samples), shadow maps for dominant lights, the simple light map. Real
  UnrealLightmass would need the editor's Swarm export (`FLightmassExporter`: scene, meshes,
  materials rendered to textures) and Swarm under Wine; the export format is in
  `UnrealEd/Src/Lightmass.cpp` / `Lightmass/Public/SceneExport.h`, so writing the export ourselves
  is possible but was not attempted. Nothing has been loaded in the game yet.
