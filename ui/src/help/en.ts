import type { HelpContent } from "./types";

/** The user guide in English. Serbian (sr.ts) has the same topics and sections. */
const en: HelpContent = {
  start: {
    title: "Getting started",
    summary: "What Zaklon is, how the laptop and the phones work together, and the first steps.",
    sections: [
      {
        id: "what",
        title: "What Zaklon is",
        body: [
          { p: "Zaklon is an offline home base for your household. It keeps knowledge (such as Wikipedia and first-aid guides), maps, your supplies at home and an AI assistant on your own laptop, and it keeps working when the internet does not." },
          { p: "There are no accounts, no cloud and no tracking. Everything stays on the laptop and on your phones." },
        ],
      },
      {
        id: "hub",
        title: "The laptop is the hub",
        body: [
          { p: "One Windows laptop (or desktop computer) runs Zaklon. We call it the **hub**: it keeps all the household's data and does the heavy work, such as downloads and the AI." },
          {
            list: [
              "Zaklon starts with Windows and keeps running when you close its window, so phones stay connected. Its icon sits in the tray, next to the clock.",
              "The tray icon's menu opens Zaklon, turns **Start with Windows** off or on, and has **Quit Zaklon**.",
              "The program and all the household's data live in the one folder you chose when installing. Uninstalling Zaklon keeps the data.",
            ],
          },
        ],
      },
      {
        id: "phones",
        title: "Phones connect over Wi-Fi",
        body: [
          { p: "Android phones get the Zaklon app from the hub itself and connect to it over your home Wi-Fi. This needs no internet, only the same Wi-Fi network." },
          {
            list: [
              "At home, a phone uses everything the hub has: the supplies, the library, the assistant and the maps.",
              "Away from home, a phone still shows its last copy of the supplies, and its shopping list keeps working. See [Working without internet](#help/offline).",
              "When there is no router (a power cut, a cabin), the laptop can make [a Wi-Fi network of its own](#help/settings/network).",
            ],
          },
        ],
      },
      {
        id: "first-steps",
        title: "First steps",
        body: [
          {
            steps: [
              "**Set up the household.** The first time Zaklon opens, choose the language, a name for the hub and a household password (at least 8 characters).",
              "**Pair the phones.** On the laptop, choose **Add a phone** on [Home](#home), or open [Settings › Devices](#settings/devices) and choose it there. See [Pairing a phone](#help/pairing).",
              "**Get the content.** In [Add-ons](#addons), the starter set downloads knowledge, first-aid and repair guides and the AI model that fits this computer with one button. This needs internet once; after that, everything works without it.",
              "**Add your supplies** in [Supplies](#supplies), and ask the [Assistant](#assistant) anything.",
            ],
          },
          { note: "Everyone who knows the household password has the same rights: they can pair a phone and change anything. Choose a password the household will remember, and keep it safe; encrypted backups need it too." },
          { p: "Moving from another computer? On the setup screen, restore the backup of your previous hub instead of setting up, and the phones keep working. See [Backups](#help/settings/backups)." },
        ],
      },
      {
        id: "finding-your-way",
        title: "Finding your way",
        body: [
          { p: "The bar at the bottom is the same on the laptop and on phones:" },
          {
            list: [
              "**Home**: the hub's state and what needs attention, such as supplies that expire soon or run low.",
              "**Assistant**: ask questions in plain words.",
              "**Tools**: every tool, sorted by topic, such as [Supplies](#supplies), [Library](#library), [Maps](#maps) and [Add-ons](#addons).",
              "**Settings**: phones, network, backups and the other settings, and this help at the end of its list.",
            ],
          },
          { p: "On the laptop, one tool can be pinned to the bar from the [Tools](#tools) screen. It then shows in the bar on every device in the household." },
          { p: "Every screen has a **How it works** link at the top that opens its page in this help." },
        ],
      },
      {
        id: "topics",
        title: "Everything sorted by topic",
        body: [
          { p: "Zaklon sorts its tools and guides into the same topics on the [Tools](#tools) screen and in the folders of [Add-ons](#addons):" },
          {
            list: [
              "**Health and first aid**: first aid, medicines and staying healthy.",
              "**Water**: finding, cleaning and storing drinking water.",
              "**Food**: recipes, storing food, canning and preserving.",
              "**Garden**: growing vegetables, fruit, herbs and microgreens, and watering them.",
              "**Power**: small solar setups, batteries and how much power you need.",
              "**Build and install**: setting up solar, drip irrigation, rainwater tanks and pumps, and repair guides.",
              "**Knowledge**: Wikipedia, books and dictionaries.",
              "**Maps**: maps of every country.",
            ],
          },
          { p: "On the Tools screen, each topic shows its tools and a **Guides for this topic** link, which opens the topic's folder in Add-ons, so the tools and the guides for a topic are in one place. A tool or a guide about more than one topic is under each of them." },
        ],
      },
    ],
  },

  pairing: {
    title: "Pairing a phone",
    summary: "Install the app on an Android phone and connect it to the hub, with the QR code or with the 6-digit code.",
    open: { href: "#settings/devices", label: "Open Settings › Devices" },
    sections: [
      {
        id: "before",
        title: "Before you start",
        body: [
          {
            list: [
              "The phone runs Android 9 or newer.",
              "The phone and the laptop are on the **same Wi-Fi network**. Mobile data does not work for this.",
              "Zaklon is running on the laptop, and you know the household password.",
            ],
          },
        ],
      },
      {
        id: "laptop",
        title: "On the laptop",
        body: [
          {
            steps: [
              "Choose **Add a phone** next to **Devices** on [Home](#home), or open [Settings › Devices](#settings/devices) and choose **Add a phone** there.",
              "The laptop shows two QR codes, one for downloading the app and one for pairing, and a 6-digit code.",
              "The codes work for 5 minutes. When they run out, choose **New code**.",
            ],
          },
        ],
      },
      {
        id: "install",
        title: "Install the app on the phone",
        body: [
          {
            steps: [
              "Scan the first QR code with the phone's camera, or type the address shown under it into the phone's browser. It looks like **http://192.168.1.10:8480/get**.",
              "Download the app and install it. Android may ask you to allow installing apps from the browser; allow it for this install.",
            ],
          },
        ],
      },
      {
        id: "qr",
        title: "Pair with the QR code (easiest)",
        body: [
          {
            steps: [
              "Open Zaklon on the phone and tap **Scan the QR code**.",
              "Scan the pairing QR code on the laptop.",
              "Enter the household password and a name for the phone, such as “Ana's phone”, and tap **Pair**.",
            ],
          },
          { p: "The QR code carries the laptop's identity, so the phone knows it is talking to your hub." },
        ],
      },
      {
        id: "code",
        title: "Pair with “Find hubs” and the 6-digit code",
        body: [
          { p: "Use this when the phone's camera cannot scan the code." },
          {
            steps: [
              "On the phone, tap **Find hubs on this network** and choose your hub from the list.",
              "Type the 6-digit code shown on the laptop.",
              "Check that the **Security code** on the phone is the same as the one on the laptop.",
              "Enter the household password and a name for the phone, and tap **Pair**.",
            ],
          },
          { note: "Before it sends the password, the phone uses the 6-digit code to make sure it is talking to your laptop and not to another device on the network. If something else answers, pairing stops and the password is not sent." },
        ],
      },
      {
        id: "tries",
        title: "Wrong code or password",
        body: [
          {
            list: [
              "A code allows three tries. After too many wrong tries, choose **New code** on the laptop.",
              "A phone that made too many wrong tries has to wait a few minutes before it can try again.",
              "The household password is never stored on the phone.",
            ],
          },
        ],
      },
      {
        id: "paired",
        title: "Paired phones",
        body: [
          {
            list: [
              "[Settings › Devices](#settings/devices) lists every paired phone and when it was last seen.",
              "On the laptop, **Remove** disconnects a phone for good. Its saved assistant conversations are deleted with it.",
              "On the phone itself, **Forget this hub** (in Settings › Devices) disconnects it; to use the hub again, pair it again.",
              "If the laptop was reinstalled or replaced, the phone says so and offers **Pair again**. Everything on the phone is kept until you do.",
            ],
          },
        ],
      },
    ],
  },

  home: {
    title: "Home",
    summary: "The household at a glance: the hub, what needs attention, the assistant, the supplies and the add-ons.",
    open: { href: "#home", label: "Open Home" },
    sections: [
      {
        id: "state",
        title: "The hub's state",
        body: [
          { p: "At the top is the hub's name and whether it is **Running** or **Not reachable**. Next to it:" },
          {
            list: [
              "**Power**: the laptop's battery and whether it is charging. A computer without a battery shows **On power**.",
              "**Devices**: how many phones are paired. On the laptop, **Add a phone** beside it shows the pairing codes right away (see [Pairing a phone](#help/pairing)).",
              "**Addresses**: where phones on the Wi-Fi reach the hub.",
            ],
          },
        ],
      },
      {
        id: "warnings",
        title: "Warnings",
        body: [
          { p: "Below the hub's state come the things that need attention, only when there are any:" },
          {
            list: [
              "The hub is not running, or the phone cannot reach it.",
              "Windows Firewall keeps phones out; a button fixes it. See [Network](#settings/network).",
              "A newer version of Zaklon is out.",
            ],
          },
        ],
      },
      {
        id: "ask",
        title: "Ask anything",
        body: [
          { p: "Type a question into **Ask anything** and press Enter: the [Assistant](#assistant) opens and asks it in a new conversation. Below the box are the last three conversations of this device; choose one to open it." },
        ],
      },
      {
        id: "lists",
        title: "Supplies",
        body: [
          { p: "**Needs attention** is one list, the most urgent first:" },
          {
            list: [
              "what has **expired** (in red), and how long ago;",
              "what **expires** within 30 days (in amber), the soonest first;",
              "what is **running low**: less than its **Warn below** amount, such as “2 of 3 kg”.",
            ],
          },
          { p: "Up to six lines show; open [Supplies](#supplies) for the rest. When nothing needs attention, the card just says **All good**." },
          { p: "Beside something expired or running low is **Add to shopping list**; what is on the list already says **On the list**. Below, **To buy** says how many things are on the shopping list and opens it." },
        ],
      },
      {
        id: "addons",
        title: "Library and add-ons",
        body: [
          { p: "How full the library's drive is, what is downloading right now, and how many add-ons have a new version, are paused or did not finish. Open [Add-ons](#addons) to manage them." },
        ],
      },
      {
        id: "tools",
        title: "Quick access",
        body: [{ p: "A tile for every tool that is not in the bar, and one for Help." }],
      },
      {
        id: "away",
        title: "On a phone away from home",
        body: [
          { p: "When the hub cannot be reached, Home shows the supplies and conversations the phone kept, with the time they are from. See [Working without internet](#help/offline)." },
        ],
      },
    ],
  },

  assistant: {
    title: "Assistant",
    summary: "Ask questions in plain words. Answers come from your library, with sources, and the assistant knows your supplies.",
    open: { href: "#assistant", label: "Open Assistant" },
    sections: [
      {
        id: "ask",
        title: "Asking a question",
        body: [
          {
            steps: [
              "Open the [Assistant](#assistant) and type your question in the box at the bottom, in English or Serbian.",
              "On the laptop, press Enter to ask (Shift+Enter starts a new line). On a phone, tap the arrow button.",
              "The assistant first looks in the library, then writes the answer. The square button stops it.",
            ],
          },
          { p: "It answers in the language you ask in. The first question after a break takes longer, because the AI has to start (up to a minute)." },
        ],
      },
      {
        id: "sources",
        title: "Sources and citations",
        body: [
          { p: "The assistant answers from the knowledge packs in your [Library](#library). Numbers in the answer, like [1], point to the articles listed under **Sources**. Tap one to read the article." },
          {
            list: [
              "When the library has nothing about the question, the answer says so. It then comes from the model's general knowledge and may be wrong.",
              "When an answer does not name its sources, a note under it asks you to check it in the articles.",
              "Under each answer you can see what it looked up and how fast it wrote.",
            ],
          },
          { warn: "The AI can make mistakes. For anything important, read the source articles." },
        ],
      },
      {
        id: "supplies",
        title: "Questions about your supplies",
        body: [
          { p: "Ask about what you have at home, for example “What expires this month?”. Such answers are marked **From your supplies**." },
          { p: "You can also ask for changes, such as “add 2 liters of milk” or “we used the rice”. The assistant shows what it would change, and nothing changes until you choose **Yes, do it**." },
        ],
      },
      {
        id: "memory",
        title: "What it remembers",
        body: [
          { p: "Say “remember that Ana is allergic to penicillin” and the assistant offers to remember it; choose **Yes, do it** to keep the note. It uses the notes when they matter for a question." },
          {
            list: [
              "The notes are under **What the assistant remembers**, at the bottom of the conversation list, and in [Settings › AI assistant](#settings/assistant/memory). You can add and delete notes there.",
              "Everyone in the household sees the same notes. The assistant keeps up to 500 notes of up to 300 characters each.",
            ],
          },
        ],
      },
      {
        id: "conversations",
        title: "Saved conversations",
        body: [
          {
            list: [
              "Every conversation is saved on the hub. The list on the left (on a phone, the list button at the top) groups them by day and has a search.",
              "**New conversation** starts a fresh one.",
              "Each device sees only its own conversations: the laptop its own, each phone its own.",
              "The **Conversation options** button (⋯) next to the title renames or deletes a conversation.",
              "Away from the hub, a phone can still read its list and the conversations it opened last, but it cannot ask the hub.",
            ],
          },
          { note: "A device keeps up to 500 conversations (the one used longest ago makes room) and up to 200 questions in one conversation. Removing a phone deletes its conversations." },
        ],
      },
      {
        id: "send",
        title: "Send to…",
        body: [
          { p: "To share a conversation, open its options, choose **Send to…** and pick the device. A copy appears there as a new conversation, marked with the name of the device that sent it. Your own conversation stays as it is." },
        ],
      },
      {
        id: "online",
        title: "Online research",
        body: [
          { p: "Under the question box is **Also search the internet**. It is off by default. When you switch it on, the assistant also searches the web (with DuckDuckGo) for this conversation only, and lists the pages it read as sources marked **(internet)**. It needs a working internet connection." },
          { note: "With the switch off, nothing you ask leaves the house." },
        ],
      },
      {
        id: "health",
        title: "Health and first aid",
        body: [
          { p: "For questions about health, injuries or medicines, the assistant is extra careful:" },
          {
            list: [
              "It keeps only what it can back with an article from the library.",
              "Under the answer it adds the emergency numbers: **194** for an ambulance in Serbia, **112** in the EU.",
              "When the library has no checked answer, it says so, gives those numbers and suggests asking a doctor or pharmacist.",
              "Notes that matter, such as an allergy, are shown above the answer.",
            ],
          },
          { warn: "Zaklon is not a doctor. In an emergency, call for help first." },
        ],
      },
      {
        id: "model",
        title: "The AI model",
        body: [
          { p: "The assistant needs an AI model, downloaded once on the laptop. When there is none, the assistant offers the one recommended for this computer. Larger models answer better but are slower and need more memory. Change the model in the Assistant itself (the **Model** box) or in [Settings › AI assistant](#settings/assistant)." },
          { p: "A model that needs more memory than this computer has is marked **needs more memory** and cannot be chosen: it would make the whole computer slow. When no model fits, the assistant is not available on this computer, and the library, maps, supplies and phones still work. When the AI says there is not enough free memory right now, close some programs and ask again." },
          { p: "The AI starts with the first question and stops by itself after 20 minutes without questions. On the laptop, **free the memory now** stops it at once." },
        ],
      },
      {
        id: "phone",
        title: "The phone's own AI",
        body: [
          { p: "At home, a phone asks the hub's AI. A phone can also keep a small model of its own for when the hub is out of reach:" },
          {
            steps: [
              "On the phone, open the Assistant and choose **AI on this phone (without the hub)**.",
              "Under **Models on the hub**, choose **Copy to phone**. This happens once, over Wi-Fi, and the screen stays on until it finishes.",
              "Choose **Start**, then ask.",
            ],
          },
          { p: "Away from the hub, the phone's own AI answers by itself. It is smaller and does not look in the library, so check what it says." },
        ],
      },
    ],
  },

  supplies: {
    title: "Supplies",
    summary: "What you have at home, where it is and when it expires, with a shopping list and barcode scanning.",
    open: { href: "#supplies", label: "Open Supplies" },
    sections: [
      {
        id: "items",
        title: "Items",
        body: [
          { p: "Supplies are shared by the whole household: a change on one device shows on all of them. The **Items** tab lists everything, with a search and a filter by category. To add something:" },
          {
            steps: [
              "Choose **Add item**.",
              "Fill in the name, the quantity and unit, the category and the place (such as Pantry or Fridge). You can add a place of your own, such as “Cabin”.",
              "Add the expiry date if it has one, and **Warn below** if you want to know when it runs low.",
              "Choose **Save**.",
            ],
          },
          { p: "The **−** and **+** buttons use one or add one. Tap an item to change it or delete it; a deleted item stays in the history." },
        ],
      },
      {
        id: "batches",
        title: "Batches and expiry dates",
        body: [
          { p: "The same thing bought at different times often has different expiry dates. Each purchase is a **batch** with its own quantity and date. Open an item to see its batches, change them or add one." },
          {
            list: [
              "The item shows the total and the earliest expiry date.",
              "Using an item takes from the batch that expires first.",
              "Dates are marked in red when expired, and in the accent color when they expire within 30 days.",
            ],
          },
        ],
      },
      {
        id: "running-low",
        title: "Running low",
        body: [
          { p: "Set **Warn below** on an item, for example 2 liters for milk. When there is less than that, the item is marked **Running low**, shows on [Home](#home) and goes on the shopping list by itself, with the amount that is missing." },
          { p: "If you delete it from the shopping list, it comes back once it has been restocked and runs low again." },
        ],
      },
      {
        id: "shopping",
        title: "Shopping list",
        body: [
          {
            list: [
              "Type in **Add to the list…** for anything else you need.",
              "At the shop, tap **Bought** for what you bought. It moves to **Put away**.",
              "**Delete** takes an entry off the list.",
            ],
          },
          { note: "The shopping list keeps working on a phone away from home. The changes wait on the phone and reach the hub when you are back." },
        ],
      },
      {
        id: "put-away",
        title: "Put away",
        body: [
          { p: "Back home, open **Put away**. For each thing you bought, check the quantity and unit, set the expiry date, the place and the category, and choose **Put away**. It is added to the stock as a new batch, or as a new item if you did not have it yet." },
        ],
      },
      {
        id: "history",
        title: "History",
        body: [
          { p: "**History** shows every change: what was added, changed, used, restocked or deleted, when, and on which device." },
        ],
      },
      {
        id: "scan",
        title: "Barcode scanning (phones)",
        body: [
          {
            steps: [
              "On a phone, choose **Scan a barcode** and point the camera at the code. The first time, allow the camera.",
              "If an item with that barcode exists, it opens. If not, a new item starts with the barcode filled in; a name the household gave the same barcode before is filled in too.",
              "In an item's form, **Scan** next to **Barcode** adds a code to that item.",
            ],
          },
          { note: "Scanning happens on the phone itself. It needs no internet and no Google services." },
        ],
      },
    ],
  },

  library: {
    title: "Library",
    summary: "Read and search knowledge packs such as Wikipedia, without internet.",
    open: { href: "#library", label: "Open Library" },
    sections: [
      {
        id: "packs",
        title: "Knowledge packs",
        body: [
          { p: "The library holds **knowledge packs**: Wikipedia, a dictionary, medical articles, first-aid, repair and gardening guides and more. Each pack is downloaded once in [Add-ons](#addons) (or imported from a USB drive), and then works without internet." },
          { p: "Until there is a pack, the Library says so and offers **Open Add-ons**." },
        ],
      },
      {
        id: "read",
        title: "Reading",
        body: [
          { p: "**Books** lists the packs on the hub. Tap one to open its start page, then follow links as on a website. **Back** returns to the Library." },
        ],
      },
      {
        id: "search",
        title: "Searching",
        body: [
          { p: "Type in **Search the library…** to search every pack at once. Each result shows a piece of the article and the pack it comes from." },
        ],
      },
      {
        id: "latin",
        title: "Serbian articles in Latin script",
        body: [
          { p: "Serbian Wikipedia is written in Cyrillic. To read it in Latin script, switch on **Latin script for Serbian articles** in [Settings › Language](#settings/language/latin). It applies to this device only." },
        ],
      },
      {
        id: "assistant",
        title: "The library and the assistant",
        body: [
          { p: "The [Assistant](#assistant) searches the same packs and names the articles it used. The more packs you have, the more it can answer." },
          { note: "Phones read the library from the hub, so they need to be on the home Wi-Fi." },
        ],
      },
    ],
  },

  maps: {
    title: "Maps",
    summary: "Offline maps of the world: the hub downloads them once, and phones show them in the CoMaps app.",
    open: { href: "#maps", label: "Open Maps" },
    sections: [
      {
        id: "how",
        title: "How it works",
        body: [
          { p: "Maps come in pieces: countries, and regions of large countries. The hub downloads the pieces you choose once, and phones get them from the hub over Wi-Fi, without internet. On phones, maps are shown by **CoMaps**, a free map app that the hub provides too." },
        ],
      },
      {
        id: "download",
        title: "Download maps on the hub",
        body: [
          {
            steps: [
              "Open [Maps](#maps) and search for a country or region.",
              "Choose **Download**. For a large country, open its regions to download only some of them.",
              "Downloads continue after interruptions. On the laptop, **Remove** deletes a map.",
            ],
          },
          { p: "The same maps are also in the **Maps** folder of [Add-ons](#addons/maps)." },
        ],
      },
      {
        id: "phone",
        title: "Maps on a phone",
        body: [
          {
            steps: [
              "Install CoMaps from the hub: on the phone, open [Maps](#maps) and tap **Install CoMaps**, or scan its QR code on the laptop's Maps screen. The app comes to the hub with the first map you download.",
              "In CoMaps, open Settings (top right), enter the address shown on the Maps screen under **Custom Map Server** and tap **Save**. On the phone, **Copy address** copies it for you.",
              "Download maps in CoMaps as usual, starting with the world overview (about 60 MB). They now come from the hub, even without internet.",
            ],
          },
          { note: "Maps downloaded in CoMaps stay on the phone, so they work away from home too." },
        ],
      },
    ],
  },

  power: {
    title: "Power calculator",
    summary: "How much battery, solar and inverter a small backup system needs for what you want to keep running in a power cut.",
    open: { href: "#power", label: "Open the power calculator" },
    sections: [
      {
        id: "how",
        title: "How it works",
        body: [
          { p: "Make a list of what should keep running when the power goes out: the refrigerator, a few lights, phones, the router. The calculator adds up the energy they use in a day and works out the battery, the solar panels and the inverter for it. It is meant for small systems that power part of the home, and it works without internet." },
          { p: "The list is kept on the hub, so everyone in the household sees the same one, on the laptop and on every phone. Anyone in the household can change it; a change is saved a moment after it is made." },
        ],
      },
      {
        id: "list",
        title: "Make the list",
        body: [
          {
            steps: [
              "Open [Power calculator](#power) under Tools. While the list is empty, **Start with a typical list** fills in a refrigerator, four LED bulbs, three phones, the router and a laptop.",
              "Pick an appliance under **Appliance to add** and choose **Add**. Each comes with a typical, careful figure; the label on your own appliance is better, so change **Power (each)** when you know it.",
              "Set **How many** and the **Use a day** in hours. Refrigerators and freezers switch on and off by themselves, so they count by their energy a day instead.",
              "For something that is not on the list, choose **Your own appliance** and give it a name, its power and its hours.",
            ],
          },
          { note: "**Power from** says whether an appliance runs through the inverter (AC: most things with a plug) or straight from the battery (DC: a 12 V lamp or a USB charger). DC saves the inverter's losses." },
        ],
      },
      {
        id: "system",
        title: "Days, battery and sun",
        body: [
          {
            list: [
              "**Days without grid**: how long the battery alone must last, even with no sun. One day covers most power cuts; three days is the usual advice for being prepared.",
              "**Battery type**: LiFePO4 (lithium iron phosphate) may be emptied to about 80–90%, lead-acid and AGM only to about 50%, so they need twice the capacity.",
              "**System voltage**: 12 V suits small systems. Bigger ones are better at 24 or 48 V, where the same power flows as less current, through thinner cables.",
              "**Sun**: the place and the month give the hours of full sun a day, from the European Commission's PVGIS data. December has the least sun, so it is chosen first; the calculator shows the panels needed in every month.",
            ],
          },
        ],
      },
      {
        id: "results",
        title: "Read the answer",
        body: [
          { p: "The answer starts with one sentence, such as \"2 × 100 Ah 12 V LiFePO4 batteries, about 1,400 W of solar panels and a 600 W pure sine wave inverter\". Below it: the battery in Ah and Wh, the panels to keep up every day and to refill an empty battery in one sunny day, the inverter's continuous and peak power, and the energy a day." },
          { p: "**How this is calculated** shows each formula with your own numbers, and **Sources** lists where the typical figures come from." },
          { note: "Refrigerators, freezers and pumps take several times their power for a moment when their motor starts. The inverter's peak (surge) rating must cover that, and the calculator says how much." },
        ],
      },
      {
        id: "safety",
        title: "Safety",
        body: [
          { warn: "Anything connected to the house wiring, plug-in \"balcony\" solar inverters too, must be connected by a licensed electrician: power fed back into the grid can kill the people repairing it. A small 12 V system with its own sockets can be a do-it-yourself project." },
          {
            list: [
              "Lithium batteries need a proper BMS (battery management system), a fuse right next to the battery and cables thick enough for the current. Charge them only between 0 °C and 40 °C.",
              "Never charge lead-acid batteries in a closed room: they give off hydrogen, which can explode.",
              "A generator runs only outdoors, at least 6 m from windows and doors, and never plugs into a socket of the house.",
            ],
          },
        ],
      },
      {
        id: "link",
        title: "Opened from a link",
        body: [
          { p: "A link can open the calculator with a list of its own, for example one from the assistant. That list is not saved: choose **Save as the household's list** to keep it, or **Show the saved list** to go back to the household's." },
        ],
      },
    ],
  },

  water: {
    title: "Water calculator",
    summary: "How much drinking water to store and how to make water safe, and how much water a vegetable garden needs with drip irrigation.",
    open: { href: "#water", label: "Open the water calculator" },
    sections: [
      {
        id: "drinking",
        title: "Drinking water to store",
        body: [
          {
            steps: [
              "Open [Water calculator](#water) under Tools. It opens on **Drinking water to store**.",
              "Set how many adults, children and pets drink from your supply, with **−** and **+** or by typing the number.",
              "Choose for how many days: **3 days**, **1 week**, **2 weeks**, or **Other** to type any number.",
            ],
          },
          { p: "The first amount is for drinking and cooking: at least one gallon (3.8 L) per person a day, as Ready.gov advises. The second adds washing and basic hygiene: at least 15 L per person a day, the minimum of the humanitarian Sphere standards. Below them are the same amounts in 5, 10 and 20 L canisters and 200 L drums." },
          { note: "Children, nursing mothers and sick people may need more water, and in very hot weather the need can double. Replace water you filled yourself every six months." },
        ],
      },
      {
        id: "safe",
        title: "Make water safe to drink",
        body: [
          { p: "Boiling is the best method: bring clear water to a rolling boil for 1 minute, or 3 minutes above 1,000 m, and let it cool. Without a way to boil it, use plain, unscented household bleach with 5–9% sodium hypochlorite: 2 drops per liter of clear water, 4 if it is cloudy, and wait at least 30 minutes." },
          { warn: "Boiling, bleach and filters cannot make water with fuel, chemicals or poison in it safe." },
          { note: "These numbers come from the CDC and the EPA, which have not endorsed Zaklon." },
        ],
      },
      {
        id: "garden",
        title: "Drip irrigation for a garden",
        body: [
          {
            steps: [
              "Choose **Drip irrigation for a garden**.",
              "For each bed, enter its length and width in meters (or choose **Area** and enter square meters) and what grows in it. **Add a bed** adds another one.",
              "Under **Weather in that month**, choose the month (the hottest one, to be safe), enter the latitude or pick a city nearby, and the usual daytime high and night low. The rain is optional.",
              "Choose the dripper spacing and flow written on your drip line.",
            ],
          },
          { p: "**What the garden needs** shows the liters a day and a week, how many drippers there are, how long to run the water each day (in the morning and the evening when it is over an hour) and a tank that holds a day's water." },
          { note: "A bucket or drum raised about 1 m can feed drip lines by gravity, but the pressure is low, so drippers give less than their rating. Hold a cup under one dripper for 10 minutes to see what it really gives, and run the water longer if needed." },
          { p: "If a local weather service gives the reference evapotranspiration (ET₀), enter it under **Your own ET₀** and it is used in place of the estimate. **How this is calculated** shows every formula and where its numbers come from." },
        ],
      },
      {
        id: "roof",
        title: "Rainwater from your roof",
        body: [
          { p: "Enter the area your roof covers, measured from above, and the month's rain under the weather. The calculator shows about how many liters the roof collects (1 mm of rain on 1 m² is 1 liter, and 80% of it is counted) and for how many days that waters the garden." },
          { warn: "Rainwater must be boiled or disinfected before drinking." },
        ],
      },
      {
        id: "shared",
        title: "Shared by the household",
        body: [
          { p: "What you enter is kept on the hub, so everyone in the household sees the same numbers, on the laptop and on every phone. Anyone in the household can change them; a change is saved a moment after it is made, and a change made on another device shows up here too. A phone away from home shows the numbers it saw last." },
        ],
      },
      {
        id: "link",
        title: "Opened from a link",
        body: [
          { p: "A link can open the calculator with numbers of its own, for example one from the assistant; what the link does not say stays as the household has it. Those numbers are not saved: choose **Save for the household** to keep them, or **Show the saved numbers** to go back to the household's." },
        ],
      },
    ],
  },

  addons: {
    title: "Add-ons",
    summary: "Download knowledge packs, AI models and maps, and copy them to or from a USB drive.",
    open: { href: "#addons", label: "Open Add-ons" },
    sections: [
      {
        id: "layout",
        title: "Drives and folders",
        body: [
          { p: "Add-ons looks like “This PC” in File Explorer." },
          {
            list: [
              "**Devices and drives** shows the drive Zaklon keeps its library on (**Zaklon library**), how full it is and how much the add-ons take. Open it to see what is on it, largest first. Its bar turns red when the drive is nearly full.",
              "On the laptop, other drives, such as a USB drive, open with copying and importing set to that drive.",
              "**Folders** hold the add-ons by topic, the same topics as on the [Tools](#tools) screen: Health and first aid, Water, Food, Garden, Power, Build and install, Knowledge and Maps, then AI models and Programs. A guide about more than one topic is in the folder of each.",
              "The search box finds add-ons and countries in every folder. The buttons next to it switch between tiles and a details table.",
            ],
          },
        ],
      },
      {
        id: "starter",
        title: "The starter set",
        body: [
          { p: "On the laptop, the starter set (**Basic pack for Serbia** or **English essentials**, by the app's language) downloads what a household needs to start with one button, **Download all**: knowledge, first-aid and repair guides, the AI model that fits this computer and, in the Serbian set, the map of Serbia." },
        ],
      },
      {
        id: "downloads",
        title: "Downloads",
        body: [
          {
            list: [
              "Choose **Download** on an add-on. The hub downloads it, even when you started it on a phone.",
              "A download can be paused and resumed. After an interruption it continues where it stopped instead of starting over.",
              "Every file is checked before it is used. A damaged file is thrown away; choose **Retry**.",
              "Downloads need at least 50% battery or the charger, and enough free disk space.",
              "Programs (the engines for the library, the assistant and maps) come along by themselves with what needs them.",
            ],
          },
        ],
      },
      {
        id: "updates",
        title: "New versions and removing",
        body: [
          { p: "When a newer version of an installed pack or map is out, it shows **New version available** and an **Update** button. On the laptop, **Remove** deletes an add-on, or an unfinished download, and frees its space." },
        ],
      },
      {
        id: "usb-copy",
        title: "Copy to a USB drive",
        body: [
          { p: "To set up another Zaklon without internet, copy your add-ons to a USB drive. On the laptop:" },
          {
            steps: [
              "Plug in the USB drive and open it under **Devices and drives**, or use **Copy to USB** at the bottom of Add-ons.",
              "Choose the add-ons to copy. You can also put Zaklon itself on the drive (the Windows installer and the phone app) for someone starting from scratch.",
              "Choose **Copy** and wait until it says **Copied to**.",
            ],
          },
          { note: "A FAT32 drive cannot hold files of 4 GB or more. For large packs, use a drive formatted as exFAT or NTFS." },
        ],
      },
      {
        id: "usb-import",
        title: "Import from a USB drive",
        body: [
          {
            steps: [
              "On the laptop, open the USB drive under **Devices and drives**, or use **Import from a USB stick or folder**.",
              "Pick the folder with the pack files (or its **zaklon-packs** subfolder) and choose **Import**.",
              "Each pack shows its progress in its folder. The files are checked before they are used.",
            ],
          },
        ],
      },
      {
        id: "phone",
        title: "On a phone",
        body: [
          { p: "A phone shows the same add-ons and can start downloads on the hub. The starter set, removing and the USB copies are on the laptop." },
        ],
      },
    ],
  },

  settings: {
    title: "Settings",
    summary: "Phones, the network, backups, the password and the other settings, category by category.",
    open: { href: "#settings", label: "Open Settings" },
    sections: [
      {
        id: "find",
        title: "Finding a setting",
        body: [
          { p: "On the laptop, [Settings](#settings) opens on **Devices**: the categories are listed on the left, and the one chosen shows beside them. On a phone the list comes first; choose a category to open it, and the arrow at the top goes back to the list." },
          { p: "**Find a setting**, at the top of the list, understands English and Serbian words, with or without accents. **Help**, at the end of the list, opens this guide. Network and Backups are only on the laptop." },
        ],
      },
      {
        id: "devices",
        title: "Devices",
        body: [
          { p: "Add a phone, and see the paired phones with when each was last seen; **Add a phone** on [Home](#home) leads here too. When Windows Firewall would keep phones out, its warning shows here as well. On a phone, **Forget this hub** is here too. See [Pairing a phone](#help/pairing)." },
        ],
      },
      {
        id: "network",
        title: "Network",
        body: [
          {
            list: [
              "**Wi-Fi network from this laptop**: when there is no router (a power cut, a cabin), the laptop can be the Wi-Fi network for the household's phones, with the Mobile hotspot built into Windows. Choose **Make the Wi-Fi network**; phones join by scanning the QR code or typing the password shown, then open Zaklon.",
              "**Windows Firewall**: when Windows would keep phones out, a warning appears with **Let phones connect**. Windows then asks the computer's administrator to confirm.",
              "**Make this network private**: when Windows treats the laptop's network as public (usual for a new Wi-Fi on Windows 11), phones on it are kept out. If it is your home network, this button in the warning tells Windows to treat it as private; Windows asks the administrator to confirm. Do not do this on a network in a café or a hotel.",
              "**Network addresses**: where phones on the same network reach the laptop.",
            ],
          },
          { note: "For its Wi-Fi network, Windows needs a connection it can share: a cable, or a Wi-Fi network it has joined before. Try it once while everything works, so you know it is ready." },
        ],
      },
      {
        id: "backups",
        title: "Backups",
        body: [
          {
            list: [
              "Every day Zaklon saves a copy of the household's data (the supplies and their history, saved conversations and notes, the paired phones and the settings) and keeps the last 7 days. The library, maps and AI models are not in backups; they come back by downloading or from USB.",
              "**Make a backup now**, or **Save a backup to a USB drive** to keep one away from the laptop.",
              "**Backup encryption**: enter the household password once, and every new backup is encrypted with it. A backup opens only with the password that was in force when it was made.",
            ],
          },
          { warn: "Do not forget the household password: without it, not even Zaklon can open an encrypted backup." },
          { p: "**Restore from a backup file**: choose the file and, for an encrypted backup, the household password from when it was made. Zaklon checks the backup, and it replaces the current data the next time Zaklon starts (**Restart Zaklon now**). Today's data is kept as a backup too. The phones paired now, the household password and the hub's identity stay as they are." },
          { p: "Moving to a new computer? Install Zaklon there and, on the setup screen, restore the old hub's backup instead of setting up. Then the paired phones, the password and the hub's identity come from the backup too, and the phones keep working." },
        ],
      },
      {
        id: "privacy",
        title: "Privacy & security",
        body: [
          {
            list: [
              "**Household password** (laptop): change it here. It is needed to pair a phone and to open an encrypted backup. Phones already paired stay connected.",
              "**Privacy**: everything stays on the hub and your phones. Zaklon goes online only when you start something that needs it, such as a download or online research, and once a day to look for a new version, which can be switched off.",
            ],
          },
        ],
      },
      {
        id: "appearance",
        title: "Appearance",
        body: [
          { p: "Choose the accent color, and a pure black background that saves battery on OLED screens. Both are remembered on this device only." },
        ],
      },
      {
        id: "language",
        title: "Language",
        body: [
          { p: "Each device chooses its own language, English or Serbian. **Latin script for Serbian articles** shows Serbian library articles in Latin script, also on this device only." },
        ],
      },
      {
        id: "assistant",
        title: "AI assistant",
        body: [
          { p: "Choose which downloaded AI model the assistant uses; the one that fits this computer is marked recommended. Models are downloaded on the laptop, in [Add-ons](#addons/models). **What the assistant remembers** is here too." },
        ],
      },
      {
        id: "updates",
        title: "Updates",
        body: [
          { p: "Once a day, when there is internet, Zaklon asks GitHub for the number of the newest version; nothing about your household is sent. **Check now** asks right away. When a newer version is out, **Open the download page** opens it in the browser." },
          { note: "Zaklon never downloads or installs an update by itself. The daily check can be switched off on the laptop." },
        ],
      },
      {
        id: "about",
        title: "About",
        body: [
          { p: "The version and the license (Zaklon is free and open source, forever), the hub's computer (processor, memory, free disk and, on the laptop, the data folder), and the licenses of the projects Zaklon is built on." },
        ],
      },
    ],
  },

  offline: {
    title: "Working without internet",
    summary: "What works with no internet or no router, and on a phone when the laptop is off or far away.",
    sections: [
      {
        id: "no-internet",
        title: "No internet",
        body: [
          { p: "Zaklon is made for this. At home, everything works without internet as long as the laptop is on and the phones are on the same Wi-Fi: the supplies, the library, the maps and the assistant." },
          { p: "Internet is needed only to download add-ons, for the assistant's online research and for the daily check for a new version." },
        ],
      },
      {
        id: "no-router",
        title: "No router",
        body: [
          { p: "Without a router (a power cut, a cabin), the laptop can make its own Wi-Fi network: open [Settings › Network](#settings/network/hotspot) and choose **Make the Wi-Fi network**. Phones join it, then open Zaklon." },
        ],
      },
      {
        id: "away",
        title: "A phone away from the hub",
        body: [
          { p: "When the laptop is off, or the phone is away from home, the phone says **The hub is out of reach** and shows how old its data is. It still has:" },
          {
            list: [
              "its last copy of the supplies, to read, and the lists on Home;",
              "the shopping list, which keeps working: add things, mark them bought or delete them;",
              "its list of saved conversations and the ones it opened last, to read;",
              "its own AI, if a model was copied to the phone (see [The phone's own AI](#help/assistant/phone));",
              "the maps already downloaded in CoMaps.",
            ],
          },
          { p: "Other changes, the library and the hub's AI need the hub." },
        ],
      },
      {
        id: "waiting",
        title: "Changes that wait",
        body: [
          { p: "Shopping list changes made away from home wait on the phone (**Changes waiting for the hub**) and are sent by themselves the next time the phone reaches the hub." },
          { note: "If the phone is paired with a different hub in the meantime, it asks whether to send the waiting changes to that hub or discard them." },
        ],
      },
      {
        id: "laptop-off",
        title: "When the laptop is off",
        body: [
          { p: "The hub runs while the laptop is on and Zaklon is running (its icon is in the tray). Closing the window does not stop it; **Quit Zaklon** in the tray menu does. Zaklon starts again with Windows." },
          { p: "Nothing is lost while the laptop is off: phones catch up when it is back." },
        ],
      },
    ],
  },

  troubleshooting: {
    title: "Troubleshooting",
    summary: "A phone cannot connect, the assistant is slow, the disk is full, and other problems.",
    sections: [
      {
        id: "connect",
        title: "A phone cannot connect",
        body: [
          {
            steps: [
              "Check that the laptop is on and Zaklon is running: its icon is in the tray, and [Home](#home) on the laptop says **Running**.",
              "Check that the phone is on the **same Wi-Fi** as the laptop, not on mobile data. A guest network often keeps devices apart; use the main one.",
              "On the laptop, open [Home](#home) or [Settings › Devices](#settings/devices). If it warns that **phones may not be able to connect**, choose **Let phones connect** and confirm in Windows.",
              "If Windows treats the network as public, phones are kept out. If it is your home network, choose **Make this network private** in the warning and confirm in Windows (or, in Windows Settings, open **Network & internet**, choose the network, and set its network profile type to **Private**).",
              "If “Find hubs” finds nothing, pair with the QR code instead.",
            ],
          },
          { p: "[Settings › Network](#settings/network/firewall) on the laptop shows whether Windows Firewall lets phones in." },
        ],
      },
      {
        id: "pairing",
        title: "Pairing does not work",
        body: [
          {
            list: [
              "**The code has expired**: choose **New code** on the laptop. A code works for 5 minutes and three tries.",
              "**Wrong household password**: check it with whoever set it up. It can be changed on the laptop, in [Settings › Privacy & security](#settings/privacy/password).",
              "**Another device answered in place of the hub**: pair by scanning the QR code instead.",
              "**The hub was reinstalled or replaced**: the phone offers **Pair again**; everything on it is kept until then.",
            ],
          },
        ],
      },
      {
        id: "slow",
        title: "The assistant is slow or stops",
        body: [
          {
            list: [
              "The first question after a break starts the AI, which takes up to a minute.",
              "A smaller AI model answers faster. Choose one in [Settings › AI assistant](#settings/assistant) or download one in [Add-ons](#addons/models).",
              "The speed is shown under each answer. If the laptop has little free memory, close other programs.",
              "An answer that takes too long is stopped. Ask again, or use a smaller model.",
              "If the AI stops while it loads, the computer may not have enough free memory: try a smaller model.",
            ],
          },
        ],
      },
      {
        id: "disk",
        title: "The disk is full",
        body: [
          {
            list: [
              "In [Add-ons](#addons), the bar of the Zaklon library drive turns red when it is nearly full. Open the drive to see what takes the most space.",
              "On the laptop, **Remove** the packs, maps and models you do not need. Paused and unfinished downloads take space too.",
              "A download that does not fit says **Not enough free disk space**.",
              "On a phone, unfinished copies of an AI model take space until you copy the model again or discard them.",
            ],
          },
        ],
      },
      {
        id: "library",
        title: "The library does not start",
        body: [
          { p: "If the Library says its engine could not start, open [Add-ons › Programs](#addons/programs), remove **Library engine (Kiwix)** and download it again." },
        ],
      },
      {
        id: "download",
        title: "A download fails",
        body: [
          {
            list: [
              "Choose **Retry**: downloads pick up where they stopped.",
              "Downloads need at least 50% battery or the charger.",
              "A damaged file is thrown away by itself; downloading it again fixes it.",
            ],
          },
        ],
      },
      {
        id: "restart",
        title: "Restarting Zaklon",
        body: [
          { p: "If the laptop says **Zaklon's hub is not running on this computer**, restart Zaklon: choose **Quit Zaklon** in the tray menu, then open Zaklon again." },
        ],
      },
    ],
  },
};

export default en;
