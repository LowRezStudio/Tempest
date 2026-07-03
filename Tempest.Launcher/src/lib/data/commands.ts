import * as m from "$lib/paraglide/messages.js";

export type Cat = { name: string; key: string; commands: string; color: string };

export const palette = [
	"hsl(0, 50%, 55%)",
	"hsl(20, 50%, 55%)",
	"hsl(40, 50%, 50%)",
	"hsl(100, 45%, 48%)",
	"hsl(140, 45%, 48%)",
	"hsl(170, 50%, 48%)",
	"hsl(200, 50%, 52%)",
	"hsl(225, 45%, 55%)",
	"hsl(250, 40%, 55%)",
	"hsl(280, 40%, 50%)",
	"hsl(310, 45%, 50%)",
	"hsl(340, 50%, 52%)",
	"hsl(15, 50%, 52%)",
	"hsl(60, 45%, 48%)",
	"hsl(150, 45%, 45%)",
	"hsl(190, 50%, 48%)",
	"hsl(270, 40%, 52%)",
	"hsl(0, 0%, 50%)",
];

export const categoryGroups: {
	label: string;
	categories: Cat[];
}[] = [
	{
		label: "Cheat Manager",
		categories: [
			{
				name: "Bot",
				key: "cm_bot",
				color: palette[0],
				commands:
					"SwitchClass, sc, SwitchWard, PurchaseGod, AddBotsToCustomMatch, JoinMatchQueue, SpawnTestBot, stb, _SpawnBot, SpawnBot, SpawnStillBot, SpawnEmoteTestBot, SpawnDeployable, SpawnEcho, ServerSpawnEcho, _SpawnTemplatePlayer, TestSkinGallery, TestPrecache, PrecacheClass",
			},
			{
				name: "Debug",
				key: "cm_debug",
				color: palette[1],
				commands:
					"ListTickableActors, TestDj, TestPanningRule, TestLanguage, ToggleLoadFailureOutput, ListAllIconReferences, ClearAllIconReferences, CrashGame, ToggleTransitionManifest, AddNotification, ResetQuestsAnimation, separator, TestServerRequestCard, Slomo, Echo, Loc, ServerExec, SimNWCondition, ToggleAIDebug, TestObstacleAvoidance, ToggleDeviceLog, ToggleCustomPhysics, ShowMoveErrors, DebugClientProjectileImpactVerification, DebugProjectileLagCompensationServer, DebugProjectileLagCompensationClient, TestSpawnPoints, SetInstantFireMeshTrace, ToggleWeaponLagPrediction, SetMaximumLagPrediction, DumpWeaponPredictionStats, DumpLastServerAims, DumpLastClientAims, ApplyDebugPropertyMod, ResetDebugPropertyMods, Adjust3pOffset, Log3pOffset, TraceDistanceAtReticle, Query, AllNoah, DBGKoga, DisplayThreatParams, ShowThreats, ReadyCapture, CaptureDone, ForceShowAmmo, DebugExtraChampionInfo, FeistyInfo, TheGoodStuff",
			},
			{
				name: "Player",
				key: "cm_player",
				color: palette[2],
				commands:
					"Energize, FillEnergy, FillEnergyAll, MaxLevel, ML, God, CharacterEnergy, DisableBaseAmmoRegen, LogAmmoRegen, energy, Cooldown, ForceToggleMount, MaxPower, HookMeUp, GiveRecommendedItems, SetGroundspeed, SetStealth, SetHealth, SetMana, DamageHealth, Heal, SetEnergy, TestStunEffect, TestStun, InvisMe, BeTheBoss, AllowHeadShots, LiveRespawn",
			},
			{
				name: "Targeting",
				key: "cm_targeting",
				color: palette[3],
				commands:
					"PossessTarget, DamageTarget, HealTarget, ShieldTarget, TargetEquipDevice, TED, TargetEquipDeviceByName, TEDBN, TargetSetMeshes, TSM, TargetSetBodyMesh, TSBM, TargetSetHeadMesh, TSHM",
			},
			{
				name: "Device",
				key: "cm_device",
				color: palette[4],
				commands:
					"RefillAmmo, SetMaxAmmo, EquipDeviceByName, EDBN, EquipDevice, ED, UnequipDevice, UD, RemoveDevice, UnequipDeviceAt, RemoveDeviceAt, RemoveAllCards, RemoveAllItems, AllocateAbilitySkillPoint, GiveCard",
			},
			{
				name: "Progression",
				key: "cm_progression",
				color: palette[5],
				commands: "AddGold, Obama, SetMeLevel, SL, GainXP, GainCredits, GainTickets",
			},
			{
				name: "Game",
				key: "cm_game",
				color: palette[6],
				commands:
					"QuickEndGame, QEG, ResetGame, rg, ChangeTaskForce, ct, ToggleTaskForce, CapturePoint, EnemyCapturePoint, ToggleCapturePointOvertime, PickPoint, SetSiegeSpeed, SetRespawnIncrease, SetDefenseRespawn, SetAttackRespawn, SetRespawnCap, SetCardCooldownIncrease, ForceRespawnAll, SkipSetup, ShowProjectileDebug, DisableProximity, ForceLanePusher, EnableScoring, DisableScoring, SetScore, EndGame, HelpMe, QuickSiege, ReinforceDoors, ReinforceSiege, SetGameEnvironmentRule, SetGameRespawnRule, SetGameMode, SetAirFriction, SetFallingFriction, SetFlyingFriction, ForceRoundSetupEnd, FRSE, NextPhase, SetCAPOvertime, PayloadForever, SetAIAccuracy, EnableThreat, EnableOcclusion, EndRound, OpenSpawnGates, CloseSpawnGates, KillAllMinions, KillAllPawnsByClass, RequestRelease, botsgod, FreezeAI, botslevel, EnableAI, KillProjectiles, ToggleAIDifficultyAdjust, StopFog, ResumeFog, SetFogDistance, RespawnFlagball, ToggleFlagballPassing, ClearPayloadTimer, KillAbyssalEcho",
			},
			{
				name: "Camera",
				key: "cm_camera",
				color: palette[7],
				commands:
					"LevelAim, Set1p, Set3p, Toggle3p, SpectatorCamera, ToggleSpectatorCamera, ShowPlayerCircles, TestShowInventory",
			},
			{
				name: "Meshes",
				key: "cm_meshes",
				color: palette[8],
				commands:
					"SetBodyMesh, SBM, SetBodyMeshByName, SBMBN, SetHeadMesh, SHM, SetHeadMeshByName, SHBN, RemoveHeadMesh, rhm, decapitate, EnableHeadMesh, ToggleHeadMesh, AllowMount, SetMountSkin, _SetMountSkin, SetVoicePreference, SetEmote, SetSpray, DAEF, DrawActorEncroachmentFire, ToggleDiminishingReturns",
			},
			{
				name: "Misc",
				key: "cm_misc",
				color: palette[9],
				commands:
					"PlayPotG, PlayPotGForAll, ResetPotG, LockPotG, PlayIntroAnim, ToggleShowAllStoreLandingItems, ToggleShowUnobtainableStoreLandingItems, ToggleShowUnfilteredStoreLandingItems, RefreshStoreLandingItems, EnableEffectLagCompensation, DisableEffectLagCompensation, FTPlayerInit, FTZombie, SetPawnLoc, BugItGo, SetVisibilityRanges, SetDamageMultiplier, SetGroundSpeedMultiplier, SetAutoHealingMultiplier, ToggleSiegeEngineRequiresAllies, SetVaultImmuneHealth, StartAutofire, ToggleTickThrottling, SetGuaranteedTickDistance, SetTickGroupCount, TestWaveform, ToggleAimAssist, SetAimAssistTargetWeightVars, ResetAimAssistValues, SetAimAssistValues, AddAimAssistKeyframe, ResetAimAssistKeyframes, StopHP5",
			},
		],
	},
	{
		label: "Player Controller",
		categories: [
			{
				name: "Native",
				key: "tpc_native",
				color: palette[10],
				commands:
					"LogPerfLeakData, TestVGSPOTG, StoreOfflineData, Bug, _Crash, LogTo, StopLogTo, ToggleInHandTargeting, DisableProfanityFilter, SpectateDamage, SpectateHeals, SpectateCrits, SpectateGold, SpectateXP, SpectateOutlines, ToggleCombatInfo, OutputRelevantActors, ToggleTick, SetPawnTickState, TgPerfTrack, DebugGetLangMsg, DumpLevelStatus, MatchLeave, SpectateGM, Spectate, SpectateStop, LiveSpectate, LiveSpectateStop, ResetKeysToDefault, OnOffhandSlotPressed, OnOffhandSlotReleased, SetAllowParticleSystems, SetAllowAnimationFrameRateLOD, ToggleClient3p, ShowDebugReticle, NextScoreboardDisplayType",
			},
			{
				name: "Input",
				key: "tpc_input",
				color: palette[11],
				commands:
					"OnLeftMousePressed, OnLeftMouseReleased, OnRightMousePressed, OnRightMouseReleased, PressJump, DoJump, HoldJump, OnJumpRelease, OnJumpHeldAltPressed, ToggleSprint, LetGoReloadWeapon, ReloadWeapon, ReloadWeaponWithFlourish, AutoMelee, OnPerCharacterAltPressed, OnDefaultCastOffhandSlotPressed, OnDefaultCastOffhandSlotReleased, OnQuickCastOffhandSlotPressed, OnQuickCastOffhandSlotReleased, OnInstantCastOffhandSlotPressed, OnFlourish, OnRespawnBeaconButtonPressed, OnRespawnBeaconButtonReleased, DropFlag",
			},
			{
				name: "Camera",
				key: "tpc_camera",
				color: palette[12],
				commands:
					"WhereAmI, TestScreenCapturePostProcess, CauseClientEvent, CCE, ResetViewOrientation, ZoomIn, ZoomOut, SetTaskforceLead, ShowBinoculars, ViewPlayerByName, DoSetViewTarget, GoSpectate, StartWatchOthers, StopWatchOthers, ToggleRove, ViewNextTeammate, ViewPreviousTeammate, Camera, SetForced3pFreeCam, FrontFacingCamera, TestShake, TestTgCameraShake, SetServerCorrectionCameraInterpVars, SkipKillCam",
			},
			{
				name: "Progression",
				key: "tpc_progression",
				color: palette[13],
				commands:
					"ClientPurchaseItem, ClientSellItem, SetAutoPurchase, SetAutoSkillUp, SetToggleZoom, SetCommandBindPC",
			},
			{
				name: "Keybinding",
				key: "tpc_keybinding",
				color: palette[14],
				commands: "UnbindCommandPC, UnbindCommandAllPC, SetBindPC, SetCommandBindPC",
			},
			{
				name: "Debug",
				key: "tpc_debug",
				color: palette[15],
				commands:
					"ShowPathToNearestPOI, SetBlur, FireDebugConsoleKismetTestNode, SetReticleColor, SetReticleRainbow, EnablePhysics, SetPhysicsWeight, FixAll, Unfix, ExecSetViewportLocationAndScale, LogLocalPropertyValue, HideMeshes, TestCrash, DumpClassInfo, IgnoreOverlays, ToggleOverlays, ToggleDetailedView, SetOutlines, SetJumpZ, SetServerFlags, SSF",
			},
			{
				name: "Misc",
				key: "tpc_misc",
				color: palette[16],
				commands:
					"SelfAlert, ServerProfileScript, RequestScoreBoard, SuppressHelpText, ResetGameTips, AllocateDevicePoint, AllocateAbilitySkillPoint, ClientPlayVGS, ClientPlayPing, ClientSurrender, ClientNotifyTutorialUIEvent, TestHelpTip, GiveGoldToFriendlyPlayer",
			},
		],
	},
	{
		label: "Engine",
		categories: [
			{
				name: "Base",
				key: "cm_base",
				color: palette[17],
				commands: "Fly, Ghost, Walk",
			},
		],
	},
];

export const signaturesRaw = `
SwitchClass(string godName, optional string SkinName, optional string weaponSkinName)
sc(string godName, optional string SkinName, optional string weaponSkinName)
SwitchWard(optional string wardSkinName)
PurchaseGod(string godName)
AddBotsToCustomMatch()
JoinMatchQueue(int nQueueId, optional int god1, optional int god2, optional int god3, optional int god4, optional int god5)
SpawnTestBot(string sName, optional string sDeviceName, optional int nFireMode = 0, optional int nTaskForce = 2, optional int nCount = 1)
stb(string sName, optional string sDeviceName, optional int nFireMode = 0, optional int nTaskForce = 2, optional int nCount = 1)
_SpawnBot(string sName, optional int nTaskForce = 2, optional int nCount = 1)
SpawnBot(string sName, optional int nTaskForce = 2, optional int nCount = 1, optional int BotDifficulty = 1, optional string BehaviorTreeName, optional int nSkinId, optional int nWeaponId)
SpawnStillBot(string sName, optional int nTaskForce = 2, optional int nCount = 1, optional int nSkinId = 0, optional int nWeaponId = 0)
SpawnEmoteTestBot(string sName, optional int nTaskForce = 2, optional int nCount = 1)
SpawnDeployable(int dep_id)
SpawnEcho(optional string sName)
ServerSpawnEcho(int nBotId)
_SpawnTemplatePlayer(int nProfileId, optional int nSkinId = 0, optional int nWeaponSkinId = 0)
TestSkinGallery(optional int nGallery = 0)
TestPrecache(optional bool bMaterialsOnly = true)
PrecacheClass(name className, optional bool bMaterialsOnly = true)
ListTickableActors()
TestDj(int SoundCueId)
TestPanningRule()
TestLanguage(optional string Langu)
ToggleLoadFailureOutput()
ListAllIconReferences()
ClearAllIconReferences()
CrashGame()
ToggleTransitionManifest()
AddNotification(string TitleString, optional string BodyString, optional string Icon, optional int GamepadKey, optional int KeyboardKey)
ResetQuestsAnimation()
separator()
TestServerRequestCard()
Slomo(float T)
Echo(string Param)
Loc(string Param)
ServerExec(string Param)
SimNWCondition(optional string ConditionName)
ToggleAIDebug(optional bool bToggle)
TestObstacleAvoidance()
ToggleDeviceLog()
ToggleCustomPhysics()
ShowMoveErrors()
DebugClientProjectileImpactVerification(int nEnable)
DebugProjectileLagCompensationServer(int nEnable)
DebugProjectileLagCompensationClient(int nEnable)
TestSpawnPoints()
SetInstantFireMeshTrace(bool bEnable)
ToggleWeaponLagPrediction()
SetMaximumLagPrediction(float MaxPrediction)
DumpWeaponPredictionStats()
DumpLastServerAims(int nNum)
DumpLastClientAims(int nNum)
ApplyDebugPropertyMod(string PropertyModString)
ResetDebugPropertyMods()
Adjust3pOffset(float OffsetX, float OffsetY, float OffsetZ)
Log3pOffset()
TraceDistanceAtReticle(float Distance)
Query(string Command)
AllNoah()
DBGKoga()
DisplayThreatParams()
ShowThreats()
ReadyCapture()
CaptureDone()
ForceShowAmmo()
DebugExtraChampionInfo(int nEnable)
FeistyInfo()
TheGoodStuff()
Energize(float pct)
FillEnergy(float pct)
FillEnergyAll(float pct)
MaxLevel()
ML()
God()
CharacterEnergy()
DisableBaseAmmoRegen(bool bDisable)
LogAmmoRegen()
energy(float pct)
Cooldown()
ForceToggleMount()
MaxPower()
HookMeUp()
GiveRecommendedItems()
SetGroundspeed(float f)
SetStealth(int Mask)
SetHealth(float f)
SetMana(float f)
DamageHealth(float f)
Heal(float f)
SetEnergy(float f)
TestStunEffect(float Duration)
TestStun(optional int nStunMask=-1)
InvisMe()
BeTheBoss(int BossLevel)
AllowHeadShots(bool bAllow)
LiveRespawn()
PossessTarget()
DamageTarget(int nDamage)
HealTarget(int nHeal)
ShieldTarget(int nShield)
TargetEquipDevice(string DeviceString)
TED(string DeviceString)
TargetEquipDeviceByName(string DeviceName)
TEDBN(string DeviceName)
TargetSetMeshes(string MeshStrings)
TSM(string MeshStrings)
TargetSetBodyMesh(string MeshStrings)
TSBM(string MeshStrings)
TargetSetHeadMesh(string MeshStrings)
TSHM(string MeshStrings)
RefillAmmo()
SetMaxAmmo(int nAmmo)
EquipDeviceByName(string DeviceName)
EDBN(string DeviceName)
EquipDevice(string DeviceString)
ED(string DeviceString)
UnequipDevice()
UD()
RemoveDevice(int nSlot)
UnequipDeviceAt(int nSlot)
RemoveDeviceAt(int nSlot)
RemoveAllCards()
RemoveAllItems()
AllocateAbilitySkillPoint()
GiveCard(string CardId)
AddGold(int Gold)
Obama(int Gold)
SetMeLevel(int Level)
SL(int Level)
GainXP(int nXP)
GainCredits(int nCredits)
GainTickets(int nTickets)
QuickEndGame()
QEG()
ResetGame()
rg()
ChangeTaskForce(coerce string Who, int TaskForce)
ct(coerce string Who, int TaskForce)
ToggleTaskForce(coerce string Who)
CapturePoint(int nIndex)
EnemyCapturePoint(int nIndex)
ToggleCapturePointOvertime(int nIndex)
PickPoint(int nIndex)
SetSiegeSpeed(float TimeBetweenPayloads)
SetRespawnIncrease(int nIncrease)
SetDefenseRespawn(float fValue)
SetAttackRespawn(float fValue)
SetRespawnCap(int nCap)
SetCardCooldownIncrease(float fValue)
ForceRespawnAll()
SkipSetup()
ShowProjectileDebug(bool bShow)
DisableProximity()
ForceLanePusher(int nLane, int nTeam)
EnableScoring()
DisableScoring()
SetScore(int nRedScore, int nBlueScore)
EndGame(int nWinningTeam)
HelpMe()
QuickSiege()
ReinforceDoors()
ReinforceSiege()
SetGameEnvironmentRule(string RuleName)
SetGameRespawnRule(string RuleName)
SetGameMode(string ModeName)
SetAirFriction(float f)
SetFallingFriction(float f)
SetFlyingFriction(float f)
ForceRoundSetupEnd()
FRSE()
NextPhase()
SetCAPOvertime(bool bEnable)
PayloadForever()
SetAIAccuracy(float fValue)
EnableThreat(bool bThreat)
EnableOcclusion(bool bEnable)
EndRound()
OpenSpawnGates()
CloseSpawnGates()
KillAllMinions()
KillAllPawnsByClass(class PawnClass)
RequestRelease()
botsgod()
FreezeAI()
botslevel(int Level)
EnableAI()
KillProjectiles()
ToggleAIDifficultyAdjust()
StopFog()
ResumeFog()
SetFogDistance(float f)
RespawnFlagball()
ToggleFlagballPassing()
ClearPayloadTimer()
KillAbyssalEcho()
LevelAim(bool bLevelAim)
Set1p()
Set3p()
Toggle3p()
SpectatorCamera(bool bEnabled)
ToggleSpectatorCamera()
ShowPlayerCircles(bool bShow)
TestShowInventory(bool bShow)
SetBodyMesh(int MeshType, int nIndex)
SBM(int MeshType, int nIndex)
SetBodyMeshByName(string MeshName)
SBMBN(string MeshName)
SetHeadMesh(int MeshType, int nIndex)
SHM(int MeshType, int nIndex)
SetHeadMeshByName(string MeshName)
SHBN(string MeshName)
RemoveHeadMesh()
rhm()
decapitate()
EnableHeadMesh()
ToggleHeadMesh()
AllowMount(bool bAllow)
SetMountSkin(string sSkin)
_SetMountSkin(string sSkin)
SetVoicePreference(string sPreference)
SetEmote(string sEmote, optional bool bReplace = false)
SetSpray(int nSprayId)
DAEF()
DrawActorEncroachmentFire()
ToggleDiminishingReturns()
PlayPotG(int nGodId, optional int nSkinId)
PlayPotGForAll(int nGodId, optional int nSkinId)
ResetPotG()
LockPotG()
PlayIntroAnim(optional bool bPlay = true)
ToggleShowAllStoreLandingItems()
ToggleShowUnobtainableStoreLandingItems()
ToggleShowUnfilteredStoreLandingItems()
RefreshStoreLandingItems()
EnableEffectLagCompensation()
DisableEffectLagCompensation()
FTPlayerInit()
FTZombie()
SetPawnLoc(float X, float Y, float Z)
BugItGo(float X, float Y, float Z, float Pitch, float Yaw, float Roll)
SetVisibilityRanges(float Near, float Far)
SetDamageMultiplier(float fMultiplier)
SetGroundSpeedMultiplier(float fMultiplier)
SetAutoHealingMultiplier(float fMultiplier)
ToggleSiegeEngineRequiresAllies(bool bRequireAllies)
SetVaultImmuneHealth(int nHealth)
StartAutofire()
ToggleTickThrottling(bool bThrottle)
SetGuaranteedTickDistance(int Distance)
SetTickGroupCount(int Count)
TestWaveform(int nWaveform)
ToggleAimAssist()
SetAimAssistTargetWeightVars(float fTargetWeightRange, float fCrosshairDistanceWeight, float fCrosshairPitchWeight, float fCrosshairYawWeight, float fMovementWeight, float fMovementDirectionWeight)
ResetAimAssistValues()
SetAimAssistValues(float fReticleSize, float fReticleX, float fReticleY, bool bShowReticle)
AddAimAssistKeyframe(float Time, float Value)
ResetAimAssistKeyframes()
StopHP5()
LogPerfLeakData(bool bOutputToLog)
TestVGSPOTG(string GodName)
StoreOfflineData(string Param)
Bug()
_Crash()
LogTo(string Param)
StopLogTo()
ToggleInHandTargeting()
DisableProfanityFilter()
SpectateDamage(bool bShow)
SpectateHeals(bool bShow)
SpectateCrits(bool bShow)
SpectateGold(bool bShow)
SpectateXP(bool bShow)
SpectateOutlines(bool bShow)
ToggleCombatInfo()
OutputRelevantActors()
ToggleTick(bool bPause)
SetPawnTickState(name State)
TgPerfTrack(int nEnable)
DebugGetLangMsg(int nMsgId)
DumpLevelStatus()
MatchLeave()
SpectateGM(string Who)
Spectate(string Who)
SpectateStop()
LiveSpectate(int nPlayerId)
LiveSpectateStop()
ResetKeysToDefault()
OnOffhandSlotPressed()
OnOffhandSlotReleased()
SetAllowParticleSystems(bool bAllow)
SetAllowAnimationFrameRateLOD(bool bAllow)
ToggleClient3p()
ShowDebugReticle()
NextScoreboardDisplayType()
OnLeftMousePressed()
OnLeftMouseReleased()
OnRightMousePressed()
OnRightMouseReleased()
PressJump()
DoJump()
HoldJump()
OnJumpRelease()
OnJumpHeldAltPressed()
ToggleSprint()
LetGoReloadWeapon()
ReloadWeapon()
ReloadWeaponWithFlourish()
AutoMelee()
OnPerCharacterAltPressed()
OnDefaultCastOffhandSlotPressed()
OnDefaultCastOffhandSlotReleased()
OnQuickCastOffhandSlotPressed()
OnQuickCastOffhandSlotReleased()
OnInstantCastOffhandSlotPressed()
OnFlourish()
OnRespawnBeaconButtonPressed()
OnRespawnBeaconButtonReleased()
DropFlag()
WhereAmI()
TestScreenCapturePostProcess()
CauseClientEvent(int EventID)
CCE(int EventID)
ResetViewOrientation(optional float Pitch, optional float Yaw)
ZoomIn()
ZoomOut()
SetTaskforceLead(string PlayerName)
ShowBinoculars()
ViewPlayerByName(string PlayerName)
DoSetViewTarget(name ViewTargetActorTag)
GoSpectate()
StartWatchOthers()
StopWatchOthers()
ToggleRove()
ViewNextTeammate()
ViewPreviousTeammate()
Camera(float FOV)
SetForced3pFreeCam(bool bEnable)
FrontFacingCamera(bool bFrontFacingCamera)
TestShake(float fDuration, float fRotAmplitude, float fRotFrequency, float fLocAmplitude, float fLocFrequency)
TestTgCameraShake(float Duration, optional float RotSinAmplitude = 120, optional float RotSinFrequency = 2, optional float LocSinAmplitude = 5, optional float LocSinFrequency = 7)
SetServerCorrectionCameraInterpVars(float InterpLocX, float InterpLocY, float InterpLocZ, float InterpRot, float InterpRotBoost)
SkipKillCam()
ClientPurchaseItem(int nItemId)
ClientSellItem(int nSlotId)
SetAutoPurchase(bool bEnabled)
SetAutoSkillUp(bool bEnabled)
SetToggleZoom(bool bEnabled)
SetCommandBindPC(int nSlot, string Cmd)
UnbindCommandPC(int nSlot)
UnbindCommandAllPC()
SetBindPC(int nSlot, string Cmd)
ShowPathToNearestPOI()
SetBlur(float BlurAmount)
FireDebugConsoleKismetTestNode()
SetReticleColor(int R, int G, int B)
SetReticleRainbow(bool bEnabled)
EnablePhysics(bool bEnable)
SetPhysicsWeight(float Weight)
FixAll()
Unfix()
ExecSetViewportLocationAndScale(float x, float y, float w, float h)
LogLocalPropertyValue(string propName)
HideMeshes(bool bHide)
TestCrash()
DumpClassInfo()
IgnoreOverlays(bool bIgnore)
ToggleOverlays()
ToggleDetailedView()
SetOutlines(bool bShow)
SetJumpZ(float F)
SetServerFlags(int nFlags)
SSF(int nFlags)
SelfAlert(string msg)
ServerProfileScript(string ScriptName, optional int nTeamIndex = -1, optional int nPlayerIndex = -1, optional string ScriptParams)
RequestScoreBoard()
SuppressHelpText()
ResetGameTips()
AllocateDevicePoint(int nCount)
AllocateAbilitySkillPoint()
ClientPlayVGS(int nVGSNode)
ClientPlayPing(int nPingId, optional vector vLoc, optional int nTargetId)
ClientSurrender(optional bool bSurrender = true)
ClientNotifyTutorialUIEvent(int Evt, int evtData)
TestHelpTip(int HelpTipId)
GiveGoldToFriendlyPlayer(int PlayerID, int GoldCount) → returns TgPlayerController.EGiveGoldResult
Fly()
Walk()
Ghost()
`.trim();

export const groupLabels: Record<string, string> = {
	"Cheat Manager": m.commands_group_cheat_manager(),
	"Player Controller": m.commands_group_tg_controller(),
	Engine: m.commands_group_cm_base(),
};

export const catLabels: Record<string, string> = {
	cm_bot: m.commands_cat_cm_bot(),
	cm_debug: m.commands_cat_cm_debug(),
	cm_player: m.commands_cat_cm_player(),
	cm_targeting: m.commands_cat_cm_targeting(),
	cm_device: m.commands_cat_cm_device(),
	cm_progression: m.commands_cat_cm_progression(),
	cm_game: m.commands_cat_cm_game(),
	cm_camera: m.commands_cat_cm_camera(),
	cm_meshes: m.commands_cat_cm_meshes(),
	cm_misc: m.commands_cat_cm_misc(),
	tpc_native: m.commands_cat_tpc_native(),
	tpc_input: m.commands_cat_tpc_input(),
	tpc_camera: m.commands_cat_tpc_camera(),
	tpc_progression: m.commands_cat_tpc_progression(),
	tpc_keybinding: m.commands_cat_tpc_keybinding(),
	tpc_debug: m.commands_cat_tpc_debug(),
	tpc_misc: m.commands_cat_tpc_misc(),
	cm_base: m.commands_cat_cm_base(),
};

export const allCategories = categoryGroups.flatMap((g) => g.categories);

export const catMap = new Map(allCategories.map((c) => [c.key, c]));

export type Cmd = { name: string; catKey: string };

export const allCommands: Cmd[] = allCategories.flatMap((cat) =>
	cat.commands.split(", ").map((cmd) => ({ name: cmd.trim(), catKey: cat.key })),
);

export const categoryKeys = allCategories.map((c) => c.key);

export function buildSigMap(raw: string): Map<string, string> {
	const map = new Map<string, string>();
	for (const line of raw.split("\n")) {
		const t = line.trim();
		const idx = t.indexOf("(");
		if (idx === -1) continue;
		const name = t.slice(0, idx).trim();
		if (!name || name.includes(" ") || name.includes("/") || name === "") continue;
		if (!map.has(name.toLowerCase())) {
			map.set(name.toLowerCase(), t);
		}
	}
	return map;
}

export const sigMap = buildSigMap(signaturesRaw);
