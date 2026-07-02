<script lang="ts">
	import { Search } from "@lucide/svelte";

	const palette = [
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

	type Cat = { name: string; key: string; commands: string; color: string };

	const categoryGroups: {
		label: string;
		categories: Cat[];
	}[] = [
		{
			label: "Cheat Manager",
			categories: [
				{
					name: "Bot", key: "cm_bot", color: palette[0],
					commands:
						"SwitchClass, sc, SwitchWard, PurchaseGod, AddBotsToCustomMatch, JoinMatchQueue, SpawnTestBot, stb, _SpawnBot, SpawnBot, SpawnStillBot, SpawnEmoteTestBot, SpawnDeployable, SpawnEcho, ServerSpawnEcho, _SpawnTemplatePlayer, TestSkinGallery, TestPrecache, PrecacheClass",
				},
				{
					name: "Debug", key: "cm_debug", color: palette[1],
					commands:
						"ListTickableActors, TestDj, TestPanningRule, TestLanguage, ToggleLoadFailureOutput, ListAllIconReferences, ClearAllIconReferences, CrashGame, ToggleTransitionManifest, AddNotification, ResetQuestsAnimation, separator, TestServerRequestCard, Slomo, Echo, Loc, ServerExec, SimNWCondition, ToggleAIDebug, TestObstacleAvoidance, ToggleDeviceLog, ToggleCustomPhysics, ShowMoveErrors, DebugClientProjectileImpactVerification, DebugProjectileLagCompensationServer, DebugProjectileLagCompensationClient, TestSpawnPoints, SetInstantFireMeshTrace, ToggleWeaponLagPrediction, SetMaximumLagPrediction, DumpWeaponPredictionStats, DumpLastServerAims, DumpLastClientAims, ApplyDebugPropertyMod, ResetDebugPropertyMods, Adjust3pOffset, Log3pOffset, TraceDistanceAtReticle, Query, AllNoah, DBGKoga, DisplayThreatParams, ShowThreats, ReadyCapture, CaptureDone, ForceShowAmmo, DebugExtraChampionInfo, FeistyInfo, TheGoodStuff",
				},
				{
					name: "Player", key: "cm_player", color: palette[2],
					commands:
						"Energize, FillEnergy, FillEnergyAll, MaxLevel, ML, God, CharacterEnergy, DisableBaseAmmoRegen, LogAmmoRegen, energy, Cooldown, ForceToggleMount, MaxPower, HookMeUp, GiveRecommendedItems, SetGroundspeed, SetStealth, SetHealth, SetMana, DamageHealth, Heal, SetEnergy, TestStunEffect, TestStun, InvisMe, BeTheBoss, AllowHeadShots, LiveRespawn",
				},
				{
					name: "Targeting", key: "cm_targeting", color: palette[3],
					commands:
						"PossessTarget, DamageTarget, HealTarget, ShieldTarget, TargetEquipDevice, TED, TargetEquipDeviceByName, TEDBN, TargetSetMeshes, TSM, TargetSetBodyMesh, TSBM, TargetSetHeadMesh, TSHM",
				},
				{
					name: "Device", key: "cm_device", color: palette[4],
					commands:
						"RefillAmmo, SetMaxAmmo, EquipDeviceByName, EDBN, EquipDevice, ED, UnequipDevice, UD, RemoveDevice, UnequipDeviceAt, RemoveDeviceAt, RemoveAllCards, RemoveAllItems, GiveCard",
				},
				{
					name: "Progression", key: "cm_progression", color: palette[5],
					commands: "AddGold, Obama, SetMeLevel, SL, GainXP, GainCredits, GainTickets",
				},
				{
					name: "Game", key: "cm_game", color: palette[6],
					commands:
						"QuickEndGame, QEG, ResetGame, rg, ChangeTaskForce, ct, ToggleTaskForce, CapturePoint, EnemyCapturePoint, ToggleCapturePointOvertime, PickPoint, SetSiegeSpeed, SetRespawnIncrease, SetDefenseRespawn, SetAttackRespawn, SetRespawnCap, SetCardCooldownIncrease, ForceRespawnAll, SkipSetup, ShowProjectileDebug, DisableProximity, ForceLanePusher, EnableScoring, DisableScoring, SetScore, EndGame, HelpMe, QuickSiege, ReinforceDoors, ReinforceSiege, SetGameEnvironmentRule, SetGameRespawnRule, SetGameMode, SetAirFriction, SetFallingFriction, SetFlyingFriction, ForceRoundSetupEnd, FRSE, NextPhase, SetCAPOvertime, PayloadForever, SetAIAccuracy, EnableThreat, EnableOcclusion, EndRound, OpenSpawnGates, CloseSpawnGates, KillAllMinions, KillAllPawnsByClass, RequestRelease, botsgod, FreezeAI, botslevel, EnableAI, KillProjectiles, ToggleAIDifficultyAdjust, StopFog, ResumeFog, SetFogDistance, RespawnFlagball, ToggleFlagballPassing, ClearPayloadTimer, KillAbyssalEcho",
				},
				{
					name: "Camera", key: "cm_camera", color: palette[7],
					commands:
						"LevelAim, Set1p, Set3p, Toggle3p, SpectatorCamera, ToggleSpectatorCamera, ShowPlayerCircles, TestShowInventory",
				},
				{
					name: "Meshes", key: "cm_meshes", color: palette[8],
					commands:
						"SetBodyMesh, SBM, SetBodyMeshByName, SBMBN, SetHeadMesh, SHM, SetHeadMeshByName, SHBN, RemoveHeadMesh, rhm, decapitate, EnableHeadMesh, ToggleHeadMesh, AllowMount, SetMountSkin, _SetMountSkin, SetVoicePreference, SetEmote, SetSpray, DAEF, DrawActorEncroachmentFire, ToggleDiminishingReturns",
				},
				{
					name: "Misc", key: "cm_misc", color: palette[9],
					commands:
						"PlayPotG, PlayPotGForAll, ResetPotG, LockPotG, PlayIntroAnim, ToggleShowAllStoreLandingItems, ToggleShowUnobtainableStoreLandingItems, ToggleShowUnfilteredStoreLandingItems, RefreshStoreLandingItems, EnableEffectLagCompensation, DisableEffectLagCompensation, FTPlayerInit, FTZombie, SetPawnLoc, BugItGo, SetVisibilityRanges, SetDamageMultiplier, SetGroundSpeedMultiplier, SetAutoHealingMultiplier, ToggleSiegeEngineRequiresAllies, SetVaultImmuneHealth, StartAutofire, ToggleTickThrottling, SetGuaranteedTickDistance, SetTickGroupCount, TestWaveform, ToggleAimAssist, SetAimAssistTargetWeightVars, ResetAimAssistValues, SetAimAssistValues, AddAimAssistKeyframe, ResetAimAssistKeyframes, StopHP5",
				},
			],
		},
		{
			label: "TgPlayerController",
			categories: [
				{
					name: "Native", key: "tpc_native", color: palette[10],
					commands:
						"LogPerfLeakData, TestVGSPOTG, StoreOfflineData, Bug, _Crash, LogTo, StopLogTo, ToggleInHandTargeting, DisableProfanityFilter, SpectateDamage, SpectateHeals, SpectateCrits, SpectateGold, SpectateXP, SpectateOutlines, ToggleCombatInfo, OutputRelevantActors, ToggleTick, SetPawnTickState, TgPerfTrack, DebugGetLangMsg, DumpLevelStatus, MatchLeave, SpectateGM, Spectate, SpectateStop, LiveSpectate, LiveSpectateStop, ResetKeysToDefault, OnOffhandSlotPressed, OnOffhandSlotReleased, SetAllowParticleSystems, SetAllowAnimationFrameRateLOD, ToggleClient3p, ShowDebugReticle, NextScoreboardDisplayType",
				},
				{
					name: "Input", key: "tpc_input", color: palette[11],
					commands:
						"OnLeftMousePressed, OnLeftMouseReleased, OnRightMousePressed, OnRightMouseReleased, PressJump, DoJump, HoldJump, OnJumpRelease, OnJumpHeldAltPressed, ToggleSprint, LetGoReloadWeapon, ReloadWeapon, ReloadWeaponWithFlourish, AutoMelee, OnPerCharacterAltPressed, OnDefaultCastOffhandSlotPressed, OnDefaultCastOffhandSlotReleased, OnQuickCastOffhandSlotPressed, OnQuickCastOffhandSlotReleased, OnInstantCastOffhandSlotPressed, OnFlourish, OnRespawnBeaconButtonPressed, OnRespawnBeaconButtonReleased, DropFlag",
				},
				{
					name: "Camera", key: "tpc_camera", color: palette[12],
					commands:
						"WhereAmI, TestScreenCapturePostProcess, CauseClientEvent, CCE, ResetViewOrientation, ZoomIn, ZoomOut, SetTaskforceLead, ShowBinoculars, ViewPlayerByName, DoSetViewTarget, GoSpectate, StartWatchOthers, StopWatchOthers, ToggleRove, ViewNextTeammate, ViewPreviousTeammate, Camera, SetForced3pFreeCam, FrontFacingCamera, TestShake, TestTgCameraShake, SetServerCorrectionCameraInterpVars, SkipKillCam",
				},
				{
					name: "Progression", key: "tpc_progression", color: palette[13],
					commands: "ClientPurchaseItem, ClientSellItem, SetAutoPurchase, SetAutoSkillUp, SetToggleZoom, SetCommandBindPC",
				},
				{
					name: "Keybinding", key: "tpc_keybinding", color: palette[14],
					commands: "UnbindCommandPC, UnbindCommandAllPC, SetBindPC, SetCommandBindPC",
				},
				{
					name: "Debug", key: "tpc_debug", color: palette[15],
					commands:
						"ShowPathToNearestPOI, SetBlur, FireDebugConsoleKismetTestNode, SetReticleColor, SetReticleRainbow, EnablePhysics, SetPhysicsWeight, FixAll, Unfix, ExecSetViewportLocationAndScale, LogLocalPropertyValue, HideMeshes, TestCrash, DumpClassInfo, IgnoreOverlays, ToggleOverlays, ToggleDetailedView, SetOutlines, SetJumpZ, SetServerFlags, SSF",
				},
				{
					name: "Misc", key: "tpc_misc", color: palette[16],
					commands:
						"SelfAlert, ServerProfileScript, RequestScoreBoard, SuppressHelpText, ResetGameTips, AllocateDevicePoint, AllocateAbilitySkillPoint, ClientPlayVGS, ClientPlayPing, ClientSurrender, ClientNotifyTutorialUIEvent, TestHelpTip, GiveGoldToFriendlyPlayer",
				},
			],
		},
	];

	const signaturesRaw = `
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
TestPrecache(int nBotId, int nSkinId, int nWeaponSkinId, optional bool bAll = false)
PrecacheClass(string godName, optional string SkinName, optional string weaponSkinName)
ListTickableActors(Object.ETickingGroup checkgroup)
TestDj()
TestPanningRule(optional string sPanningRule = "Headphone")
TestLanguage(string newLangExt)
ToggleLoadFailureOutput()
ListAllIconReferences(optional bool bIncludeManifests = false)
ClearAllIconReferences()
CrashGame()
ToggleTransitionManifest()
AddNotification(optional int nQuantity = 1, optional byte eQuestOrigin = 0, optional byte eType = 1, optional int dwLootId = 19767)
ResetQuestsAnimation(optional int nReduce = 10000000)
separator()
TestServerRequestCard(int nDeviceId, int nLevel)
Slomo(float T)
Echo(string inputString)
Loc()
ServerExec(string FSCommand)
SimNWCondition(int nPktLoss, int nPktLag)
ToggleAIDebug(optional bool bAttachAIDebugger = false)
TestObstacleAvoidance(optional int TestType)
ToggleDeviceLog(optional TgObject.TG_EQUIP_POINT eqp = 0)
ToggleCustomPhysics()
ShowMoveErrors()
DebugClientProjectileImpactVerification()
DebugProjectileLagCompensationServer()
DebugProjectileLagCompensationClient()
TestSpawnPoints(optional float PauseTime = 3.0)
SetInstantFireMeshTrace(bool bEnabled)
ToggleWeaponLagPrediction()
SetMaximumLagPrediction(float NewMaximum)
DumpWeaponPredictionStats()
DumpLastServerAims()
DumpLastClientAims()
ApplyDebugPropertyMod(string sProperty, bool bPercent, float fModifier1, optional float fModifier2 = 0.0, optional float fModifier3 = 0.0, optional float fModifier4 = 0.0, optional float fModifier5 = 0.0, optional float fModifier6 = 0.0)
ResetDebugPropertyMods(optional string sProperty)
Adjust3pOffset(int X, int Y, int Z)
Log3pOffset()
TraceDistanceAtReticle()
Query(optional string sType)
AllNoah(optional string ChannelName)
DBGKoga()
DisplayThreatParams()
ShowThreats(bool bEnabled, optional string ChannelName)
ReadyCapture()
CaptureDone()
ForceShowAmmo()
DebugExtraChampionInfo()
FeistyInfo()
TheGoodStuff()
Energize()
FillEnergy()
FillEnergyAll(optional float fPercent)
MaxLevel()
ML()
God()
CharacterEnergy()
DisableBaseAmmoRegen()
LogAmmoRegen()
energy()
Cooldown()
ForceToggleMount()
MaxPower()
HookMeUp()
GiveRecommendedItems()
SetGroundspeed(float val)
SetStealth(bool bOn)
SetHealth(int Health)
SetMana(int mana)
DamageHealth(optional int DamageAmount)
Heal(optional int HealAmount)
SetEnergy(float Value)
TestStunEffect(float fDuration)
TestStun(Controller.EStunType Type)
InvisMe(bool bInvis)
BeTheBoss()
AllowHeadShots(optional bool bEnable = true)
LiveRespawn(optional bool bResetHealth = true, optional bool bResetDevices = true)
PossessTarget()
DamageTarget(optional int DamageAmount = -1)
HealTarget(optional int HealAmount = -1)
ShieldTarget(optional int Amount = 1000)
TargetEquipDevice(int nDeviceId, int nEquipPointId, optional int nLevel = 0, optional int FireMode = 0)
TED(int nDeviceId, int nEquipPointId, optional int nLevel = 0, optional int FireMode = 0)
TargetEquipDeviceByName(string sDeviceName, int nEquipPointId, optional int nLevel = 0, optional int FireMode = 0)
TEDBN(string sDeviceName, int nEquipPointId, optional int nLevel = 0, optional int FireMode = 0)
TargetSetMeshes(int nBodyMeshID, optional int nHeadMeshID = 0)
TSM(int nBodyMeshID, optional int nHeadMeshID = 0)
TargetSetBodyMesh(int nBodyMeshID)
TSBM(int nBodyMeshID)
TargetSetHeadMesh(int nHeadMeshID)
TSHM(int nHeadMeshID)
RefillAmmo()
SetMaxAmmo(int MaxAmmo)
EquipDeviceByName(string sDeviceName, int nEquipPointId, optional int nLevel = 1, optional int FireMode = 0)
EDBN(string sDeviceName, int nEquipPointId, optional int nLevel = 1, optional int FireMode = 0)
EquipDevice(int nDeviceId, int nEquipPointId, optional int nLevel = 1, optional int FireMode = 0)
ED(int nDeviceId, int nEquipPointId, optional int nLevel = 1, optional int FireMode = 0)
UnequipDevice(int nDeviceId)
UD(int nDeviceId)
RemoveDevice(int nDeviceId)
UnequipDeviceAt(int nEquipPointId)
RemoveDeviceAt(int nDeviceId)
RemoveAllCards()
RemoveAllItems()
GiveCard(int nDeviceId)
AddGold(int nCurrency)
Obama(int nCurrency)
SetMeLevel(int nLevel)
SL(int nLevel)
GainXP(int xpAmount)
GainCredits(int creditsAmount)
GainTickets(int ticketsAmount)
QuickEndGame(optional bool bWin = true)
QEG(optional bool bWin = true)
ResetGame()
rg()
ChangeTaskForce(byte nTaskForce)
ct(byte nTaskForce)
ToggleTaskForce()
CapturePoint()
EnemyCapturePoint()
ToggleCapturePointOvertime()
PickPoint(optional int Index = -1)
SetSiegeSpeed(float fSpeed)
SetRespawnIncrease(float fIncrease)
SetDefenseRespawn(float fDuration)
SetAttackRespawn(float fDuration)
SetRespawnCap(float fDuration)
SetCardCooldownIncrease(float fIncrease)
ForceRespawnAll()
SkipSetup()
ShowProjectileDebug(bool bEnabled)
DisableProximity(bool bEnabled)
ForceLanePusher()
EnableScoring()
DisableScoring()
SetScore(optional int tf1Score = 0, optional int tf2Score = 0)
EndGame()
HelpMe()
QuickSiege()
ReinforceDoors(optional int healthAmt = 20000000)
ReinforceSiege(optional int healthAmt = 20000000)
SetGameEnvironmentRule(TgObject.EGameEnvironmentRule Rule)
SetGameRespawnRule(TgObject.EGameRespawnRule Rule)
SetGameMode(TgObject.EGameMode Mode)
SetAirFriction(float frictionAmt)
SetFallingFriction(float frictionAmt)
SetFlyingFriction(float frictionAmt)
ForceRoundSetupEnd()
FRSE()
NextPhase()
SetCAPOvertime(float overtimeDuration, optional float overtimeWarning = -1.0)
PayloadForever()
SetAIAccuracy(float DegreeMissed, float MissLikelihood)
EnableThreat(bool bEnabled)
EnableOcclusion(bool bEnabled)
EndRound(optional int nTaskForce = 0)
OpenSpawnGates()
CloseSpawnGates()
KillAllMinions()
KillAllPawnsByClass(string PawnClassName)
RequestRelease()
botsgod(optional bool bOn = true)
FreezeAI(optional bool bOn = true)
botslevel(int nLevel)
EnableAI(bool bEnabled)
KillProjectiles()
ToggleAIDifficultyAdjust()
StopFog()
ResumeFog()
SetFogDistance(int Distance)
RespawnFlagball()
ToggleFlagballPassing()
ClearPayloadTimer()
KillAbyssalEcho()
LevelAim()
Set1p(optional bool bForce1P = true)
Set3p(optional bool bForce3P = true)
Toggle3p()
SpectatorCamera(optional bool bOn = true)
ToggleSpectatorCamera()
ShowPlayerCircles(bool bEnabled)
TestShowInventory()
SetBodyMesh(int nBodyMeshID)
SBM(int nBodyMeshID)
SetBodyMeshByName()
SBMBN()
SetHeadMesh(int nHeadMeshID)
SHM(int nHeadMeshID)
SetHeadMeshByName()
SHBN()
RemoveHeadMesh()
rhm()
decapitate()
EnableHeadMesh(bool bEnabled)
ToggleHeadMesh()
AllowMount(optional bool bEnabled = true)
SetMountSkin(int nSkinId)
_SetMountSkin(int nSkinId)
SetVoicePreference(string sPreference)
SetEmote(int nSlot, int nEmoteId)
SetSpray(int nSlot, int nSprayId)
DAEF(optional bool bDraw = true)
DrawActorEncroachmentFire(optional bool bDraw = true)
ToggleDiminishingReturns()
PlayPotG()
PlayPotGForAll()
ResetPotG()
LockPotG(bool bLocked)
PlayIntroAnim()
ToggleShowAllStoreLandingItems()
ToggleShowUnobtainableStoreLandingItems()
ToggleShowUnfilteredStoreLandingItems()
RefreshStoreLandingItems()
EnableEffectLagCompensation()
DisableEffectLagCompensation()
FTPlayerInit()
FTZombie(bool bEnabled)
SetPawnLoc(float X, float Y, float Z)
BugItGo(coerce float X, coerce float Y, coerce float Z, coerce int Pitch, coerce int Yaw, coerce int Roll)
SetVisibilityRanges(optional float fNormal = -1.0, optional float fInVolume = -1.0)
SetDamageMultiplier(float NewMult)
SetGroundSpeedMultiplier(float NewMult)
SetAutoHealingMultiplier(float NewMult)
ToggleSiegeEngineRequiresAllies()
SetVaultImmuneHealth(float fHealth)
StartAutofire(optional float fTime = 1.0, optional bool bStart = true)
ToggleTickThrottling()
SetGuaranteedTickDistance(float fDistance)
SetTickGroupCount(int nTickGroupCount)
TestWaveform(ForceFeedbackWaveform FFWaveform, optional float fScale = 1.0, optional float fScaleDuration = 1.0)
ToggleAimAssist()
SetAimAssistTargetWeightVars(float CurrentTargetBonus, float NotCurrentTargetPenalty, float MinAccuracyBonus, float MaxAccuracyBonus, float MinInaccuracyPenalty, float MaxInaccuracyPenalty, float MaxWeight)
ResetAimAssistValues()
SetAimAssistValues(float MagnetScaleX, float MagnetScaleY, float FrictionScaleX, float FrictionScaleY, float TrackingScaleX, float TrackingScaleY)
AddAimAssistKeyframe(string keyframeType, string KeyframeName, float Distance)
ResetAimAssistKeyframes(string keyframeType)
StopHP5()
LogPerfLeakData()
TestVGSPOTG(int nPackId, int nBotId, int nSkinId)
StoreOfflineData()
Bug()
_Crash()
LogTo(string fsHost, optional int nPort = 4040)
StopLogTo()
ToggleInHandTargeting(bool bHide)
DisableProfanityFilter(bool bDisable)
SpectateDamage(bool bShow)
SpectateHeals(bool bShow)
SpectateCrits(bool bShow)
SpectateGold(bool bShow)
SpectateXP(bool bShow)
SpectateOutlines(bool bShow)
ToggleCombatInfo()
OutputRelevantActors()
ToggleTick(string ClassName, bool bDisable)
SetPawnTickState(int nState)
TgPerfTrack(bool bStart)
DebugGetLangMsg(int nMsgId)
DumpLevelStatus()
MatchLeave(bool bAsTeam)
SpectateGM(string sPlayerName, optional string sSpectatePassword)
Spectate(string sPlayerName, optional string sSpectatePassword)
SpectateStop()
LiveSpectate(string sPlayerNameInstanceId)
LiveSpectateStop()
ResetKeysToDefault(bool bSpectator)
OnOffhandSlotPressed(TgObject.TG_EQUIP_POINT eqp) → returns bool
OnOffhandSlotReleased(TgObject.TG_EQUIP_POINT eqp) → returns bool
SetAllowParticleSystems(bool Value)
SetAllowAnimationFrameRateLOD(bool Value, optional float lLODDistanceFactor, optional int lLODFrameRate)
ToggleClient3p()
ShowDebugReticle(string Mode, optional string Arg1, optional string Arg2, optional string Arg3, optional string Arg4)
NextScoreboardDisplayType()
OnLeftMousePressed()
OnLeftMouseReleased()
OnRightMousePressed()
OnRightMouseReleased()
PressJump(bool bOn)
DoJump()
HoldJump()
OnJumpRelease()
OnJumpHeldAltPressed()
ToggleSprint()
LetGoReloadWeapon()
ReloadWeapon()
ReloadWeaponWithFlourish()
AutoMelee(bool bEnabled)
OnPerCharacterAltPressed()
OnDefaultCastOffhandSlotPressed(TgObject.TG_EQUIP_POINT eqp) → returns bool
OnDefaultCastOffhandSlotReleased(TgObject.TG_EQUIP_POINT eqp) → returns bool
OnQuickCastOffhandSlotPressed(TgObject.TG_EQUIP_POINT eqp) → returns bool
OnQuickCastOffhandSlotReleased(TgObject.TG_EQUIP_POINT eqp) → returns bool
OnInstantCastOffhandSlotPressed(TgObject.TG_EQUIP_POINT eqp) → returns bool
OnFlourish()
OnRespawnBeaconButtonPressed()
OnRespawnBeaconButtonReleased()
DropFlag()
WhereAmI()
TestScreenCapturePostProcess()
CauseClientEvent(optional name EventName)
CCE(optional name EventName)
ResetViewOrientation()
ZoomIn()
ZoomOut()
SetTaskforceLead(string fsName)
ShowBinoculars(bool bShow)
ViewPlayerByName(string PlayerName)
DoSetViewTarget(Actor NewTarget)
GoSpectate()
StartWatchOthers(optional TgPlayerController.EWatchOtherPlayersMode Mode = 1)
StopWatchOthers()
ToggleRove()
ViewNextTeammate()
ViewPreviousTeammate()
Camera(name NewMode)
SetForced3pFreeCam(bool bEnabled)
FrontFacingCamera(bool bEnabled)
TestShake(int Index)
TestTgCameraShake(TgCameraShake CameraShake)
SetServerCorrectionCameraInterpVars(float SnapDist, float MinBaseSpeed, float MaxBaseSpeed, float BaseSpeedMult, float NewInfoWeight, float MinCorrectionMod, float MaxCorrectionMod, float CorrectionModMult)
SkipKillCam()
ClientPurchaseItem(int nLootTableId, int nLootTableItemId, int nItemCount)
ClientSellItem(int nInventoryId)
SetAutoPurchase(bool bAutoPurchase)
SetAutoSkillUp(bool bAutoSkill)
SetToggleZoom(bool bToggleZoom)
UnbindCommandPC(bool bSpectator, bool bGamepad, int nAlternate, string Command)
UnbindCommandAllPC(bool bSpectator, string Command)
SetBindPC(bool bSpectator, string ExtendedBinding, string Command)
SetCommandBindPC(bool bSpectator, bool bGamepad, int nAlternate, string ExtendedBinding, string Command)
ShowPathToNearestPOI()
SetBlur(float TargetBlurAmount, float InterpSpeed, float BlurKernelSize)
FireDebugConsoleKismetTestNode()
SetReticleColor(float R, float G, float B, optional float A = 1.0)
SetReticleRainbow(optional bool bSetActive = true)
EnablePhysics(bool bEnabled)
SetPhysicsWeight(float Amount)
FixAll()
Unfix(name BoneName)
ExecSetViewportLocationAndScale(optional float OriginX = 0.0, optional float OriginY = 0.0, optional float SizeX = 1.0, optional float SizeY = 1.0)
LogLocalPropertyValue(int nPropId)
HideMeshes(bool bHide1PMesh, bool bHide3PMesh)
TestCrash()
DumpClassInfo(string sClassName)
IgnoreOverlays(bool bIgnore)
ToggleOverlays()
ToggleDetailedView()
SetOutlines(bool bFriendly, bool bEnemy)
SetJumpZ(float NewJumpZ)
SetServerFlags(optional bool bSprint = true, optional bool bMinimapTeamVisibility = false, optional float DamageMultiplier = 1.0, optional bool bForce3P = false, optional bool bForce1P = false, optional bool bOutOfCombatSprint = false)
SSF(optional bool bSprint = true, optional bool bMinimapTeamVisibility = false, optional float DamageMultiplier = 1.0, optional bool bForce3P = false, optional bool bForce1P = false, optional bool bOutOfCombatSprint = false)
SelfAlert(int nPriority, float fDuration, int nMsgId)
ServerProfileScript(string Command)
RequestScoreBoard()
SuppressHelpText()
ResetGameTips()
AllocateDevicePoint(int nDeviceId)
AllocateAbilitySkillPoint(TgObject.TG_EQUIP_POINT eqp)
ClientPlayVGS(int nId, optional int usedVPSetting = 0)
ClientPlayPing(float X, float Y, float Z, TgObject.PING_TYPE pingType)
ClientSurrender(optional bool bSurrender = true)
ClientNotifyTutorialUIEvent(int Evt, int evtData)
TestHelpTip(int HelpTipId)
GiveGoldToFriendlyPlayer(int PlayerID, int GoldCount) → returns TgPlayerController.EGiveGoldResult
`.trim();

	function buildSigMap(raw: string): Map<string, string> {
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

	const sigMap = buildSigMap(signaturesRaw);
	sigMap.set("onrightmousepressed (climbing)", "OnRightMousePressed()");
	sigMap.set("onleftmousepressed (climbing)", "OnLeftMousePressed()");
	sigMap.set("onleftmousepressed (dead)", "OnLeftMousePressed()");
	sigMap.set("onrightmousepressed (dead)", "OnRightMousePressed()");
	sigMap.set("viewplayerbyname (spectating)", "ViewPlayerByName(string PlayerName)");
	sigMap.set("prevweapon (spectating)", "PrevWeapon()");
	sigMap.set("nextweapon (spectating)", "NextWeapon()");
	sigMap.set("onrightmousepressed (spectating)", "OnRightMousePressed()");

	const allCategories = categoryGroups.flatMap((g) => g.categories);
	const catMap = new Map(allCategories.map((c) => [c.key, c]));

	type Cmd = { name: string; catKey: string };

	const allCommands: Cmd[] = allCategories.flatMap((cat) =>
		cat.commands.split(", ").map((cmd) => ({ name: cmd.trim(), catKey: cat.key })),
	);

	const categoryKeys = allCategories.map((c) => c.key);

	let enabledKeys = $state(new Set(categoryKeys));
	let search = $state("");

	function toggleCategory(key: string) {
		if (enabledKeys.has(key)) {
			if (enabledKeys.size === 1) return;
			enabledKeys.delete(key);
		} else {
			enabledKeys.add(key);
		}
		enabledKeys = new Set(enabledKeys);
	}

	const visible = $derived(
		allCommands.filter(
			(cmd) =>
				enabledKeys.has(cmd.catKey) &&
				(search === "" || cmd.name.toLowerCase().includes(search.toLowerCase())),
		),
	);

	const count = $derived(visible.length);

	let expandedKey = $state<string | null>(null);
	let descriptions = $state<Record<string, string>>({});

	function getSig(name: string): string {
		return sigMap.get(name.toLowerCase()) ?? `${name}()`;
	}

	function toggleExpanded(key: string) {
		expandedKey = expandedKey === key ? null : key;
	}
</script>

<div class="space-y-3">
	<label class="input input-bordered flex items-center gap-2">
		<Search size={16} class="opacity-50 shrink-0" />
		<input
			type="text"
			placeholder="Search commands..."
			class="grow"
			bind:value={search}
		/>
		{#if count !== allCommands.length}
			<span class="text-xs text-base-content/50 shrink-0">{count}/{allCommands.length}</span>
		{/if}
	</label>

	{#each categoryGroups as group}
		<div>
			<span class="text-xs font-semibold uppercase tracking-wider text-base-content/40 mb-1.5 block">
				{group.label}
			</span>
			<div class="flex flex-wrap gap-x-3 gap-y-1.5">
				{#each group.categories as { name, color, key }}
					<label class="flex items-center gap-1.5 cursor-pointer select-none text-sm group">
						<input
							type="checkbox"
							checked={enabledKeys.has(key)}
							onchange={() => toggleCategory(key)}
							class="hidden"
						/>
						<span
							class="inline-flex items-center justify-center w-3.5 h-3.5 rounded transition-colors shrink-0"
							style={`
								background-color: ${enabledKeys.has(key) ? color : 'transparent'};
								border: 2px solid ${color};
							`}
						>
							{#if enabledKeys.has(key)}
								<svg class="w-2.5 h-2.5 text-white" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="4" stroke-linecap="round" stroke-linejoin="round">
									<polyline points="20 6 9 17 4 12" />
								</svg>
							{/if}
						</span>
						<span style="color: {color}">{name}</span>
					</label>
				{/each}
			</div>
		</div>
	{/each}

	{#if visible.length > 0}
		<div class="flex flex-wrap gap-1.5 items-start">
			{#each visible as cmd (cmd.name + cmd.catKey)}
				{@const cat = catMap.get(cmd.catKey)!}
				{@const key = cmd.name + cmd.catKey}
				{@const expanded = expandedKey === key}
				<!-- svelte-ignore a11y_click_events_have_key_events a11y_no_noninteractive_element_interactions -->
				<div
					class="cmd-badge cursor-pointer"
					class:expanded
					role="button"
					tabindex="0"
					onclick={(e) => {
						if ((e.target as HTMLElement).tagName === "TEXTAREA") return;
						toggleExpanded(key);
					}}
					onkeydown={(e) => {
						if ((e.target as HTMLElement).tagName === "TEXTAREA") return;
						if (e.key === 'Enter' || e.key === ' ') toggleExpanded(key);
					}}
					style={`
						--cat-color: ${cat.color};
						--cat-bg: color-mix(in srgb, ${cat.color} 18%, transparent);
						--cat-border: color-mix(in srgb, ${cat.color} 40%, transparent);
					`}
				>
					{#if expanded}
						<div class="flex flex-col gap-1.5">
							<div class="flex items-center gap-2">
								<span class="font-mono text-xs font-bold shrink-0">{cmd.name}</span>
								<span class="text-[10px] text-base-content/30 bg-base-300 px-1.5 py-0.5 rounded font-mono">{cat.name}</span>
							</div>
							<div class="bg-base-300/50 rounded-lg p-2 overflow-x-auto">
								<code class="text-[11px] font-mono leading-relaxed whitespace-pre-wrap break-all">{getSig(cmd.name)}</code>
							</div>
							<textarea
								class="w-full text-xs bg-base-300/50 border border-base-300 rounded px-2 py-1 resize-none outline-none focus:outline-none leading-tight"
								rows={1}
								placeholder="Add a description..."
								value={descriptions[key] ?? ""}
								oninput={(e) => {
									descriptions[key] = e.currentTarget.value;
								}}
							></textarea>
						</div>
					{:else}
						<span class="font-mono text-xs">{cmd.name}</span>
					{/if}
				</div>
			{/each}
		</div>
	{:else}
		<div class="flex flex-col items-center justify-center h-48 gap-3 text-base-content/50">
			<Search size={40} class="opacity-30" />
			<p class="text-lg">No commands matching "{search}"</p>
		</div>
	{/if}
</div>

<style>
	.cmd-badge {
		display: inline-flex;
		align-items: center;
		border-radius: 0.5rem;
		border: 1px solid var(--cat-border);
		background-color: var(--cat-bg);
		color: var(--cat-color);
		transition: color 120ms, background-color 120ms, border-color 120ms, transform 120ms, padding 120ms, border-radius 120ms;
		padding: 0.25rem 0.625rem;
	}
	.cmd-badge:not(.expanded):hover {
		background-color: color-mix(in srgb, white 20%, transparent);
		border-color: white;
		color: white;
		transform: scale(1.05);
	}
	.cmd-badge:not(.expanded):active {
		transform: scale(0.95);
	}
	.cmd-badge.expanded {
		background-color: color-mix(in srgb, var(--cat-color) 6%, var(--fallback-b2, oklch(var(--b2))));
		border-color: var(--cat-border);
		padding: 0.5rem;
		border-radius: 0.5rem;
		animation: cmd-expand-in 150ms ease-out;
	}
	.cmd-badge.expanded textarea {
		color: white;
		min-height: 0;
	}
	.cmd-badge.expanded textarea::placeholder {
		color: rgba(255, 255, 255, 0.45);
	}
	.cmd-badge.expanded textarea:focus {
		outline: none !important;
		border-color: var(--cat-border);
	}

	@keyframes cmd-expand-in {
		from {
			opacity: 0;
			transform: scale(0.95);
		}
		to {
			opacity: 1;
			transform: scale(1);
		}
	}
</style>
