# UnrealScript: matinee

[Agent entry point](../AGENTS.md). Read only the sections relevant to the task.

Paths in this reference retain their original project/repository context; verify details against
current sources.

### Matinee (`matinee.rs`, `matinee_curve.rs`)

What cooking keeps (survey of every level package of the 8.1 client: 423 `SeqAct_Interp`, 302
`InterpData`, 413 `InterpGroup` + 158 `InterpGroupDirector`; read with `workspace.search {isA}` +
`object.get`): track classes `InterpTrackMove` 383, `Director` 148, `FloatProp` 145 (FOVAngle 133,
Brightness, Radius; 127 of them store no `FloatTrack` at all, i.e. an empty curve), `AnimControl` 77
(`AnimSeqs` keyed by `StartTime`, not `Time`), `MoveAxis` 54 (9 split move tracks × 6), `Sound` 44
(`Sounds[] {Time, Sound, Volume, Pitch}` + a `VectorTrack`), `VectorMaterialParam` 20, `Fade` 19,
`Event` 13, `Toggle` 10, Hi-Rez's `InterpTrackAkEvent` 10 (`AkEvents[] {Time, Event}`),
`FloatMaterialParam` 7, `ColorProp` 6 (LightColor), `VectorProp` 3 (DrawScale3D). None of
Visibility, ParticleReplay, Slomo, LinearColorProp, Notify, FaceFX, HeadTracking. Curve points keep
everything: `InVal`, `OutVal`, `ArriveTangent`, `LeaveTangent`, `InterpMode` (move tracks:
CurveAutoClamped 1194, Linear 326, CurveAuto 406, CurveUser 56; vector material params also
CurveBreak), and 397 move keys have non-zero tangents. No curve stores `InterpMethod` (so
`IMT_UseFixedTangentEvalAndNewAutoTangents`). Every move track stores its `LookupTrack` (all group
names None); no track sets `RotMode`, `LookAtGroupName`, `bUseQuatInterpolation`,
`bUseRawActorTMforRelativeToInitial` or the curve tensions; `MoveFrame` is `IMF_RelativeToInitial`
on 340 of 370 (the rest the default `IMF_World`); `MoveAxis` is left out when it is the first value
(`AXIS_TranslationX`). Editor-only data is gone: no `InterpCurveEdSetup`, no `InterpFilter`, no
`PathBuildTime`, no `EdSectionStart` (only `EdSectionEnd`, 164), no `bShouldBakeAndPrune`, no track
instances (`InterpGroupInst`/`InterpTrackInst` are runtime). `InterpLength` is absent on 57 data
(the class default 5.0). Groups: `GroupName`, `GroupColor`, `InterpTracks`, `GroupAnimSets` (77),
`bIsParented` (20), `bIsFolder` (5); director groups usually leave `GroupName` at `DirGroup`.
Actions: `InputLinks` (5, or 6 with Hi-Rez's "Reset", which `Default__SeqAct_Interp` of 8.1 has),
`OutputLinks` (Completed, Reversed + one per event name), `VariableLinks` (Data + one per group,
`SeqVar_Object`, 0..255), and the flags `bRewindOnPlay`, `bLooping`, `PlayRate`,
`bForceStartPos`/`ForceStartPosition`, `bClientSideOnly`, `bReversePlayback`,
`bRewindIfAlreadyPlaying`, `bNoResetOnRewind`. Matinee objects have no archetype in the export
table, flags `Transactional|LoadForClient|LoadForServer|LoadForEdit` and no native part (properties
only); `SeqVar_Object`/`InterpData` store `ObjInstanceVersion 1`, `ParentSequence`,
`DrawWidth/DrawHeight 32`. `UInterpTrackMove::GetLocationAtTime` in the 8.2 server binary matches
the source (RotMode 1 = look at group, 2 = ignore). All 423 actions read with `matinee.get`.

Editing (one undo step each, unsaved edits of the level): keys by property path with the engine's
invariants: a move track's `PosTrack`/`EulerTrack`/`LookupTrack` (an axis'
`FloatTrack`/`LookupTrack`) gain, lose and retime keys together (`linked`), a retimed key is moved
to keep the lists sorted (`SetKeyframeTime(bUpdateOrder)`), every curve change runs
`AutoSetTangents` (tension `LinCurveTension`/`AngCurveTension`/`CurveTension`; linear colour curves
are not clamped, as the engine has no clamping overload for them), setting a tangent of an auto key
makes it `CIM_CurveUser`; an empty curve (not stored) is added as a property first. New keys on
curves take the value the curve has there (`eval`) and `CIM_CurveAutoClamped`. Groups (`add_group`,
unique names, one director group which gets its director track), tracks (`add_track`: refused like
the editor for `bDirGroupOnly`/`bOnePerGroup`/`bSubTrackOnly` from the class defaults; move tracks
`IMF_RelativeToInitial` + a zero key at 0 as `SetTrackToSensibleDefault`/`AddKeyframe` do; property
tracks `PropertyName` + `TrackTitle`, material ones `ParamName`), properties of
action/data/group/track (`set_props`), connectors kept in step by a port of
`USeqAct_Interp::UpdateConnectorsFromData` (`sync_connectors`; group renames rename the connector so
its links stay, and rewrite director cuts' `TargetCamGroup` and `LookAtGroupName`), `connect` (a
`SeqVar_Object` holding the actor, reused when the sequence has one, linked through `kismet::link`),
and new Matinees (`matinee.create`: a `SeqAct_Interp` copied from the palette's example and stripped
to its connectors, `init_new`, plus a new `InterpData` linked to "Data"). New objects are
property-only exports with a lower-case class import, NetIndex -1. Checked on copies of SG_Bazaar_P
and TD_Abyss_P (key add/retime/reorder, modes, tangents, groups, 8 track classes, event outputs
added and removed, connect, rename, new Matinee with a director group, cut and fade): `upk verify` 0
mismatches, `matinee.get` of the saved file gives the same timeline, ten undos return Abyss to
clean. Not in game yet; a new Matinee exists only in the client package, so its movement is not
replicated unless the server's map has it (`bClientSideOnly` is the honest setting for cosmetic
ones), and static actors (StaticMeshActor, collection meshes) do not move in game.

Evaluation (`matinee_curve.rs`): `eval` = `FInterpCurve::Eval` (Hermite `CubicInterp` with tangents
× segment length, constant/linear segments), `auto_set_tangents` + `clamp_float_tangent` =
`AutoSetTangents` / `ComputeCurveTangent` / `FClampFloatTangent`, `key_transform` =
`GetKeyTransformAtTime` (no lookup groups or quaternion slerp: unused by the game), `initial_tm` =
`CalcInitialTransform` (the actor's frame less the track's transform at the start position unless
`bUseRawActorTMforRelativeToInitial`), `world_transform` = `ComputeWorldSpaceKeyTransform` with
`GetMoveRefFrame` (no base). The Studio's `matinee-math.ts` is a line-by-line port; on the test
curves (`cargo test matinee_curve -- --nocapture` prints them) the two agree exactly (189 values,
max difference 0).
