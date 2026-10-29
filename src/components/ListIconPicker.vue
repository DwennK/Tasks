<script setup lang="ts">
import { computed, nextTick, onMounted, ref } from "vue";
import {
  PhSmiley,
  PhPawPrint,
  PhHamburger,
  PhSoccerBall,
  PhBuildings,
  PhLightbulb,
  PhHash,
  PhFlag,
  PhMagnifyingGlass,
  PhCaretDown,
} from "@phosphor-icons/vue";

defineProps<{ value: string }>();
const emit = defineEmits<{ select: [icon: string]; close: [] }>();
type Emoji = { icon: string; name: string };
const inventory = (source: string): Emoji[] =>
  source
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean)
    .map((line) => {
      const split = line.indexOf(" ");
      return { icon: line.slice(0, split), name: line.slice(split + 1) };
    });
const categories = [
  {
    id: "people",
    name: "People & Body",
    icon: PhSmiley,
    items: inventory(`
😀 grinning happy smile face
😃 smiling eyes happy face
😄 grinning smiling happy face
😁 beaming smiling teeth grin
😆 laughing squinting face
😅 sweat smiling relieved face
🤣 rolling laughing rofl
😂 tears joy laugh
🙂 slightly smiling face
🙃 upside down face
😉 winking face
😊 blushing smiling eyes
😇 halo angel innocent
🥰 smiling hearts love
😍 heart eyes love
🤩 star eyes excited
😘 blowing kiss love
😗 kissing face
😚 kissing closed eyes
😙 kissing smiling eyes
😋 delicious savor food
😛 tongue face
😜 winking tongue silly
🤪 zany crazy face
😝 squinting tongue
🤑 money mouth rich
🤗 hugging face
🤭 hand mouth giggle
🤫 shushing quiet
🤔 thinking face
🤐 zipper mouth secret
🤨 raised eyebrow skeptical
😐 neutral face
😑 expressionless face
😶 no mouth silent
😏 smirking face
😒 unamused face
🙄 rolling eyes
😬 grimacing face
🤥 lying nose
😌 relieved face
😔 pensive sad
😪 sleepy tired
🤤 drooling face
😴 sleeping zzz
😷 face mask sick
🤒 thermometer fever
🤕 bandage hurt
🤢 nauseated sick
🤮 vomiting sick
🤧 sneezing sick
🥵 hot face
🥶 cold face
🥴 woozy dizzy
😵 dizzy crossed eyes
🤯 exploding mind blown
🤠 cowboy hat
🥳 party celebration
😎 sunglasses cool
🤓 nerd glasses
🧐 monocle face
😕 confused face
😟 worried face
🙁 slightly frowning sad
☹️ frowning sad
😮 surprised open mouth
😯 hushed face
😲 astonished shocked
😳 flushed embarrassed
🥺 pleading puppy eyes
😦 frowning open mouth
😧 anguished face
😨 fearful scared
😰 anxious sweat
😥 sad relieved disappointed
😢 crying sad
😭 loudly crying tears
😱 screaming fear
😖 confounded face
😣 persevering face
😞 disappointed face
😓 downcast sweat
😩 weary tired
😫 tired face
🥱 yawning sleepy
😤 steam nose triumph
😡 angry pouting
😠 angry face
🤬 swearing symbols
👍 thumbs up like
👎 thumbs down dislike
👏 clapping applause
🙌 raised hands celebration
👋 waving hello hand
🤝 handshake agreement
💪 muscle strength
🙏 folded hands thanks
👀 eyes look
🧠 brain mind
🫶 heart hands love
🧑 person adult
👩 woman
👨 man
👶 baby child
👩‍💻 woman technologist developer
👨‍💻 man technologist developer
🧑‍🎨 artist creative
🧑‍🚀 astronaut space`),
  },
  {
    id: "nature",
    name: "Animals & Nature",
    icon: PhPawPrint,
    items: inventory(`
🐶 dog puppy
🐱 cat kitten
🐭 mouse
🐹 hamster
🐰 rabbit bunny
🦊 fox
🐻 bear
🐼 panda
🐻‍❄️ polar bear
🐨 koala
🐯 tiger
🦁 lion
🐮 cow
🐷 pig
🐸 frog
🐵 monkey
🐔 chicken
🐧 penguin
🐦 bird
🦉 owl
🦋 butterfly
🐝 bee honey
🐞 ladybug
🐢 turtle
🐬 dolphin
🐳 whale
🐠 tropical fish
🦄 unicorn
🐴 horse
🐕 dog
🐈 cat
🌸 cherry blossom flower
🌹 rose flower
🌻 sunflower
🌷 tulip flower
🌱 seedling growth plant
🌿 herb leaf
🍀 clover luck
🌵 cactus
🌳 tree deciduous
🌲 evergreen tree
🍁 maple leaf autumn
🍂 fallen leaves autumn
☀️ sun sunny
🌤️ sun cloud weather
🌧️ rain cloud
⛈️ thunderstorm
❄️ snow snowflake
🌈 rainbow
🔥 fire flame
🌊 wave water
🌙 moon night
⭐ star
✨ sparkles magic
🌍 earth globe europe africa
🌎 earth globe americas
🌏 earth globe asia`),
  },
  {
    id: "food",
    name: "Food & Drink",
    icon: PhHamburger,
    items: inventory(`
🍏 green apple fruit
🍎 red apple fruit
🍐 pear fruit
🍊 orange tangerine fruit
🍋 lemon citrus
🍌 banana fruit
🍉 watermelon fruit
🍇 grapes fruit
🍓 strawberry fruit
🫐 blueberries fruit
🍒 cherry fruit
🍑 peach fruit
🥭 mango fruit
🍍 pineapple fruit
🥑 avocado
🥦 broccoli vegetable
🥕 carrot vegetable
🌽 corn vegetable
🥔 potato
🍞 bread
🥐 croissant breakfast
🥖 baguette bread
🧀 cheese
🥚 egg breakfast
🥓 bacon breakfast
🍔 hamburger burger
🍟 fries potato
🍕 pizza
🌮 taco
🌯 burrito wrap
🥙 kebab stuffed pita
🥗 salad
🍝 pasta spaghetti
🍜 noodles ramen
🍣 sushi
🍚 rice
🥟 dumpling
🍰 cake dessert
🎂 birthday cake
🍪 cookie
🍩 doughnut donut
🍫 chocolate
🍦 ice cream
🍿 popcorn
☕ coffee tea hot
🍵 tea green
🧃 juice box
🥤 soda drink
💧 water drop
🍺 beer
🍷 wine
🥂 champagne toast`),
  },
  {
    id: "activities",
    name: "Activities",
    icon: PhSoccerBall,
    items: inventory(`
⚽ football soccer sport
🏀 basketball sport
🏈 american football sport
⚾ baseball sport
🎾 tennis sport
🏐 volleyball sport
🏉 rugby sport
🥏 flying disc frisbee
🎱 billiards pool
🏓 table tennis ping pong
🏸 badminton
🏒 hockey ice
⛳ golf
🏹 archery bow arrow
🎣 fishing
🥊 boxing glove
🥋 martial arts
⛸️ ice skating
🎿 skiing
🛷 sled winter
🏆 trophy achievement
🏅 medal winner
🥇 first gold medal
🎯 target darts goal
🎮 video game controller
🕹️ joystick gaming
🎲 dice game
🧩 puzzle piece
🎨 artist palette art
🎭 theater performing arts
🎬 clapper film movie
🎤 microphone singing
🎧 headphones music
🎼 musical score
🎹 piano keyboard
🎸 guitar music
🎺 trumpet music
🥁 drum music
🎪 circus tent
🎟️ ticket event
🏃 running exercise
🚴 cycling bicycle sport
🏊 swimming exercise
🧘 yoga meditation`),
  },
  {
    id: "travel",
    name: "Travel & Places",
    icon: PhBuildings,
    items: inventory(`
🚗 car automobile
🚕 taxi
🚙 suv car
🚌 bus
🚎 trolleybus
🏎️ racing car
🚓 police car
🚑 ambulance
🚒 fire engine truck
🚚 delivery truck
🚲 bicycle bike
🛵 scooter
🏍️ motorcycle
🚆 train railway
🚇 subway metro
🚋 tram streetcar
✈️ airplane flight travel
🚀 rocket launch space
🚁 helicopter
⛵ sailboat boat
🚢 ship cruise
⚓ anchor
⛽ fuel gas station
🚦 traffic light
🗺️ world map travel
🗽 statue liberty new york
🗼 tokyo tower
🏰 castle
🏯 japanese castle
🏟️ stadium
🎡 ferris wheel
🏖️ beach vacation
🏝️ island tropical
🏔️ mountain snowy
⛰️ mountain peak
🏕️ camping tent
🏠 house home personal
🏡 house garden
🏢 office building business
🏬 department store
🏥 hospital
🏦 bank
🏨 hotel
🏫 school education
🏭 factory industrial
⛪ church
🕌 mosque
⛩️ shrine
🌉 bridge night
🌃 city night
🌅 sunrise
🌇 sunset city`),
  },
  {
    id: "objects",
    name: "Objects",
    icon: PhLightbulb,
    items: inventory(`
⌚ watch time
📱 mobile phone
💻 laptop computer development
🖥️ desktop computer
⌨️ keyboard
🖨️ printer
🖱️ mouse computer
💾 disk save
💿 cd disc
📷 camera photo
🎥 video camera
📺 television tv
📻 radio
🎙️ studio microphone podcast
🔋 battery energy
🔌 plug electric
💡 bulb idea light
🔦 flashlight torch
🕯️ candle
🗑️ wastebasket trash
💰 money bag
💳 credit card payment
💎 diamond jewel
⚖️ scales balance law
🔧 wrench tool
🔨 hammer tool
🛠️ tools repair
⚙️ gear settings
🔗 link chain
🔑 key access
🔒 lock secure
🔓 unlock
🎁 gift present
🎈 balloon party
🎉 party popper celebration
📦 package box delivery
✉️ envelope email mail
📮 mailbox post
📚 books reading
📖 open book
📝 memo note writing
📒 notebook
📋 clipboard checklist
📅 calendar schedule
📌 pushpin pinned
📎 paperclip attachment
✂️ scissors
📏 ruler measurement
🔍 search magnifier
🧪 test tube science
🔬 microscope science
💊 medicine pill
🩺 stethoscope health
🧹 broom cleaning
🛏️ bed sleep
🪴 potted plant`),
  },
  {
    id: "symbols",
    name: "Symbols",
    icon: PhHash,
    items: inventory(`
❤️ red heart love
🧡 orange heart
💛 yellow heart
💚 green heart
💙 blue heart
💜 purple heart
🖤 black heart
🤍 white heart
🤎 brown heart
💔 broken heart
💕 two hearts
💯 hundred perfect
✅ check mark done complete
☑️ check box
✔️ check mark
❌ cross no cancel
❎ cross button
➕ plus add
➖ minus remove
✖️ multiply
➗ divide
❓ question help
❗ exclamation important
‼️ double exclamation
⚠️ warning caution
🚫 prohibited forbidden
♻️ recycle environment
🔄 refresh repeat
🔁 repeat loop
🔂 repeat once
🔔 bell notification
🔕 mute bell
💬 speech comment chat
💭 thought bubble
🗨️ speech bubble
📣 announcement megaphone
🔴 red circle
🟠 orange circle
🟡 yellow circle
🟢 green circle
🔵 blue circle
🟣 purple circle
⚫ black circle
⚪ white circle
🔷 blue diamond
🔶 orange diamond
🔺 red triangle
🔻 down triangle
▶️ play
⏸️ pause
⏹️ stop
🔼 up arrow
🔽 down arrow
➡️ right arrow
⬅️ left arrow
⬆️ up arrow
⬇️ down arrow
♾️ infinity
✳️ asterisk star
©️ copyright
®️ registered`),
  },
  {
    id: "flags",
    name: "Flags",
    icon: PhFlag,
    items: inventory(`
🏁 checkered flag finish
🚩 triangular red flag
🎌 crossed flags
🏴 black flag
🏳️ white flag
🏳️‍🌈 rainbow pride flag
🏳️‍⚧️ transgender flag
🇨🇭 switzerland swiss flag
🇫🇷 france french flag
🇩🇪 germany german flag
🇮🇹 italy italian flag
🇦🇹 austria flag
🇧🇪 belgium flag
🇳🇱 netherlands dutch flag
🇱🇺 luxembourg flag
🇪🇸 spain spanish flag
🇵🇹 portugal flag
🇬🇧 united kingdom british england flag
🇮🇪 ireland flag
🇩🇰 denmark flag
🇸🇪 sweden flag
🇳🇴 norway flag
🇫🇮 finland flag
🇮🇸 iceland flag
🇵🇱 poland flag
🇨🇿 czech flag
🇬🇷 greece flag
🇺🇦 ukraine flag
🇺🇸 united states usa america flag
🇨🇦 canada flag
🇲🇽 mexico flag
🇧🇷 brazil flag
🇦🇷 argentina flag
🇨🇱 chile flag
🇨🇴 colombia flag
🇦🇺 australia flag
🇳🇿 new zealand flag
🇯🇵 japan flag
🇨🇳 china flag
🇰🇷 south korea flag
🇮🇳 india flag
🇸🇬 singapore flag
🇹🇭 thailand flag
🇻🇳 vietnam flag
🇮🇩 indonesia flag
🇿🇦 south africa flag
🇲🇦 morocco flag
🇪🇬 egypt flag
🇰🇪 kenya flag
🇳🇬 nigeria flag
🇪🇺 european union europe flag`),
  },
];
const activeCategory = ref("people");
const query = ref("");
const search = ref<HTMLInputElement>();
const grid = ref<HTMLElement>();
const currentCategory = computed(
  () =>
    categories.find((category) => category.id === activeCategory.value) ||
    categories[0]!,
);
const allEmojis = categories.flatMap((category) => category.items);
const visible = computed(() => {
  const text = query.value.trim().toLocaleLowerCase();
  if (!text) return currentCategory.value.items;
  const terms = text.split(/\s+/);
  return allEmojis.filter(
    (item) =>
      item.icon.includes(text) ||
      terms.every((term) => item.name.includes(term)),
  );
});
function chooseCategory(id: string) {
  activeCategory.value = id;
  query.value = "";
  nextTick(() => {
    if (grid.value) grid.value.scrollTop = 0;
  });
}
function random() {
  const values = visible.value.length ? visible.value : allEmojis;
  const seed = new Uint32Array(1);
  crypto.getRandomValues(seed);
  const item = values[seed[0]! % values.length];
  if (item) emit("select", item.icon);
}
function navigate(event: KeyboardEvent) {
  const elements =
    grid.value?.querySelectorAll<HTMLButtonElement>(".emoji-choice");
  if (!elements?.length) return;
  const index = [...elements].indexOf(
    document.activeElement as HTMLButtonElement,
  );
  if (index < 0) return;
  const steps: Record<string, number> = {
    ArrowLeft: -1,
    ArrowRight: 1,
    ArrowUp: -11,
    ArrowDown: 11,
  };
  let target: number;
  if (event.key === "Home")
    target = event.ctrlKey || event.metaKey ? 0 : Math.floor(index / 11) * 11;
  else if (event.key === "End")
    target =
      event.ctrlKey || event.metaKey
        ? elements.length - 1
        : Math.min(elements.length - 1, Math.floor(index / 11) * 11 + 10);
  else if (event.key in steps)
    target = Math.max(
      0,
      Math.min(elements.length - 1, index + steps[event.key]!),
    );
  else return;
  event.preventDefault();
  event.stopPropagation();
  elements[target]?.focus();
}
function searchKey(event: KeyboardEvent) {
  if (event.key === "ArrowDown") {
    event.preventDefault();
    grid.value?.querySelector<HTMLButtonElement>(".emoji-choice")?.focus();
  }
  if (event.key === "Enter" && visible.value[0]) {
    event.preventDefault();
    emit("select", visible.value[0].icon);
  }
}
onMounted(() => nextTick(() => search.value?.focus()));
</script>

<template>
  <section
    class="list-icon-picker"
    role="dialog"
    aria-label="Choose list icon"
    @keydown.esc.stop.prevent="emit('close')"
    @click.stop
  >
    <label class="icon-search"
      ><PhMagnifyingGlass :size="18" /><input
        ref="search"
        v-model="query"
        placeholder="Search"
        aria-label="Search icons"
        @keydown="searchKey"
    /></label>
    <div class="icon-picker-toolbar">
      <div class="icon-categories" role="tablist" aria-label="Icon categories">
        <button
          v-for="category in categories"
          :key="category.id"
          type="button"
          role="tab"
          :aria-selected="activeCategory === category.id && !query"
          :aria-label="category.name"
          :title="category.name"
          :class="{ selected: activeCategory === category.id && !query }"
          @click="chooseCategory(category.id)"
        >
          <component :is="category.icon" :size="18" />
        </button>
      </div>
      <span class="picker-divider"></span
      ><button type="button" class="picker-action" @click="random">
        Random</button
      ><button
        type="button"
        class="picker-action"
        :disabled="!value"
        @click="emit('select', '')"
      >
        Reset
      </button>
    </div>
    <div class="icon-category-heading">
      <span>{{ query ? "Search Results" : currentCategory.name }}</span
      ><PhCaretDown v-if="!query" :size="12" />
    </div>
    <div
      ref="grid"
      class="emoji-grid"
      role="group"
      aria-label="Icons"
      @keydown="navigate"
    >
      <button
        v-for="item in visible"
        :key="item.icon"
        type="button"
        class="emoji-choice"
        :aria-label="item.name"
        :aria-pressed="value === item.icon"
        :title="item.name"
        @click="emit('select', item.icon)"
      >
        {{ item.icon }}
      </button>
      <p v-if="!visible.length" class="empty-icon-search">No matching icons</p>
    </div>
  </section>
</template>

<style scoped>
.list-icon-picker {
  width: 398px;
  max-width: calc(100vw - 32px);
  padding: 12px 12px 10px;
  border-radius: 14px;
  background: var(--popover, #fff);
  color: var(--text, #252525);
  box-shadow: 0 5px 17px #0004;
  position: relative;
  font-family:
    -apple-system, BlinkMacSystemFont, "Helvetica Neue", Arial, sans-serif;
  z-index: 90;
  box-sizing: border-box;
}
.icon-search {
  height: 32px;
  display: flex;
  align-items: center;
  gap: 11px;
  border: 1px solid #8f98b4;
  border-radius: 8px;
  padding: 0 8px;
  margin-bottom: 15px;
  color: #d2d2d5;
  box-shadow: 0 0 0 0.3px #354471;
}
.icon-search input {
  border: 0;
  background: none;
  outline: none;
  min-width: 0;
  flex: 1;
  font: inherit;
  font-size: 14px;
  color: var(--text, #262626);
  height: 100%;
  padding: 0;
}
.icon-search input::placeholder {
  color: #ceced1;
}
.icon-picker-toolbar {
  display: flex;
  align-items: center;
  height: 24px;
  gap: 9px;
  margin-bottom: 7px;
}
.icon-categories {
  display: flex;
  align-items: center;
  gap: 9px;
  flex: 1;
  min-width: 0;
}
.icon-categories button {
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 0;
  flex-shrink: 0;
  width: 19px;
  height: 24px;
  color: var(--text, #262626);
  border-radius: 4px;
  border: 0;
  background: none;
  cursor: pointer;
}
.icon-categories button.selected {
  color: #3d4977;
}
.icon-categories button:hover {
  background: var(--hover, #f2f2f4);
}
.picker-divider {
  width: 1px;
  height: 24px;
  background: var(--border, #e5e5e8);
  flex-shrink: 0;
}
.picker-action {
  font: inherit;
  font-size: 13px;
  color: #aaa;
  padding: 0;
  border: 0;
  background: none;
  cursor: pointer;
  white-space: nowrap;
}
.picker-action:hover {
  color: var(--text, #262626);
}
.picker-action:disabled {
  opacity: 0.35;
  cursor: default;
}
.icon-category-heading {
  display: flex;
  align-items: center;
  gap: 7px;
  line-height: 20px;
  font-size: 13px;
  color: #a3a3a7;
  margin-bottom: 7px;
  padding-left: 1px;
}
.emoji-grid {
  display: grid;
  grid-template-columns: repeat(11, minmax(0, 1fr));
  grid-auto-rows: 30px;
  max-height: 210px;
  overflow: auto;
  overscroll-behavior: contain;
  scrollbar-width: thin;
  align-items: center;
  justify-items: center;
  margin: 0 -3px;
  padding: 0 1px;
}
.emoji-choice {
  font-family:
    "Apple Color Emoji", "Segoe UI Emoji", "Noto Color Emoji", sans-serif;
  font-size: 19px;
  line-height: 25px;
  width: 29px;
  height: 29px;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 0;
  border: 0;
  border-radius: 5px;
  background: none;
  cursor: pointer;
  outline: none;
}
.emoji-choice:hover,
.emoji-choice[aria-pressed="true"] {
  background: var(--hover, #eeeff3);
}
.emoji-choice:focus-visible,
.icon-categories button:focus-visible,
.picker-action:focus-visible {
  outline: 2px solid #6474a3;
  outline-offset: 0;
}
.empty-icon-search {
  grid-column: 1/-1;
  text-align: center;
  font-size: 12px;
  color: var(--muted, #999);
  padding: 25px 5px;
  margin: 0;
}
.emoji-grid::-webkit-scrollbar {
  width: 4px;
}
.emoji-grid::-webkit-scrollbar-thumb {
  background: #a7a7b044;
  border-radius: 4px;
}
@media (max-width: 430px) {
  .list-icon-picker {
    padding: 10px;
    width: calc(100vw - 32px);
  }
  .icon-picker-toolbar {
    gap: 5px;
  }
  .icon-categories {
    gap: 5px;
  }
  .icon-categories button {
    width: 17px;
  }
  .icon-categories svg {
    width: 16px;
  }
  .picker-action {
    font-size: 11px;
  }
  .emoji-choice {
    width: 26px;
    height: 28px;
    font-size: 18px;
  }
}
</style>
