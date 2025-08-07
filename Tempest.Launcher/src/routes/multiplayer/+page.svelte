<script lang="ts">
import Input from "$lib/components/ui/Input.svelte";
import Button from "$lib/components/ui/Button.svelte";
import Dropdown from "$lib/components/ui/DropdownChampions.svelte";
import championsData from '$lib/assets/champions/champions.json';

let selectedChampion = $state(championsData[0]);
let selectedTalent = $state(championsData[0].defaultTalent);
const isChampionModalVisible = $state(false);

// There's probably a billion better ways to do this it was for testing
function getChampionClassIcon(championClass: string) {
    const classIcons = {
        'Damage': '',
        'Flank': '',
        'Tank': 'https://static.wikia.nocookie.net/paladins_gamepedia/images/4/49/Class_Front_Line_Icon.png',
        'Support': 'https://static.wikia.nocookie.net/paladins_gamepedia/images/d/dc/Class_Support_Icon.png',
    };

    switch (championClass) {
        case 'Damage':
           return classIcons.Damage;
        case 'Flank':
            return classIcons.Flank;
        case 'Tank':
            return classIcons.Tank;
        case 'Support':
            return classIcons.Support;
    }
}

function showModal() {
    //isChampionModalVisible.value = true;
}

function hideModal() {
    //isChampionModalVisible.value = false;
}
</script>
<div class="multiplayer-area">
    <div class="player-infos">
        sfsf
        <Input placeholder="Username" maxlength=20 style="width: 85%; height: 40px;"/>
        <!--<img id="champion-banner" src={selectedChampion.test} alt="Ying" />-->

    </div>

    <div class="champion-custom">
        <div class="champion-banner">
        <!--<img id="champion-banner" src={selectedChampion.test} alt="Ying" />-->
            <img id="champion-banner" src={selectedChampion.banner} alt="Ying" />
            <hr>
        </div>
        <div class="skin-section">
            <div class="skin-category">
                <Button style="font-size: 14px;">Skin</Button>
                <Button style="font-size: 14px;">Weapon</Button>
                <Button style="font-size: 14px;">Head</Button>
                <Button style="font-size: 14px;">Voicepack</Button>
                <Button style="font-size: 14px;">Horse</Button>
                <Button style="font-size: 14px;">Spray</Button>
            </div>
        </div>
        <hr>
    </div>
    
    <div class="champion-infos">
        <div class="playing-as">
            Playing as <br>
        </div>
        <div class="champion-name">
            <Dropdown
            title={selectedChampion.name}
            icon={selectedChampion.icon}
            subtitle={selectedChampion.class}
            subicon={getChampionClassIcon(selectedChampion.class)}
            items={championsData}
            selectedChampion={(selection: any) => {selectedChampion = selection}}>
            </Dropdown>
        </div>
        <hr>
        <div class="champion-talents">
            <!-- Maybe a better way to do this?-->
            <div class="triangle-talent-top">
                <img src={selectedChampion.talents[0].icon} id={ selectedTalent === selectedChampion.talents[0].name ? "selected" : "" } class={selectedChampion.talents[0].icon} onclick={selectedTalent = selectedChampion.talents[0].name} alt="">
            </div>
            <div class="triangle-talent-bottom">
                <img src={selectedChampion.talents[1].icon} id={ selectedTalent === selectedChampion.talents[1].name ? "selected" : "" } class={selectedChampion.talents[1].icon} onclick={selectedTalent = selectedChampion.talents[1].name} alt="">
                <img src={selectedChampion.talents[2].icon} id={ selectedTalent === selectedChampion.talents[2].name ? "selected" : "" } class={selectedChampion.talents[2].icon} onclick={selectedTalent = selectedChampion.talents[2].name} alt="">

            </div>                
        </div>
        <hr>
    </div>
</div>

<style>
    .multiplayer-area {
        display: flex;
        flex-direction: row;
        height: 100%;
        gap: 10px;

    }
    .player-infos {
        background-color: var(--bg-surface);
		display: flex;
		flex-direction: column;
        align-items: center;
		width: 25%;
        height: 100%;
        gap: 10px;
		overflow: hidden;
    }
    .champion-banner {
        min-height: 50%;
        max-height: 50%;
    }
    .champion-custom {
        background-color: var(--bg-surface);
		display: flex;
		flex-direction: column;
        align-items: center;
		width: 60%;
        height: 100%;
		overflow: hidden;
    }
    .skin-section {
        display: flex;
        justify-content: space-evenly;
        align-items: center;
        font-weight: 600;
        height: 20%;
    }
    .champion-infos {
        position: relative;
        background-color: var(--bg-surface);
		display: flex;
		flex-direction: column;
        align-items: center;
		width: 25%;
        height: 100%;
		overflow: hidden;
    }
    .playing-as {
        font-weight: 700;
        width: 100%;
        text-align: left;
        padding: 10px 15px;
    }

    .champion-name {
        margin-bottom: 15px;
    }

    hr {
        width: 100%;
        height: 1px;
        border-color: var(--color-primary);
    }
    .champion-talents {
        display: flex;
        flex-direction: row;
        justify-content: center;
        align-items: center;
        margin: 10px auto;
    }
</style>