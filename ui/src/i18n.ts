export type Lang = "en" | "sr";

const en = {
  home: "Home", library: "Library", maps: "Maps", supplies: "Supplies", assistant: "Assistant",
  addons: "Add-ons", household: "Household", more: "More",
  hubStatus: "Hub", online: "Running", offline: "Not reachable", devices: "Devices", uptime: "Uptime",
  version: "Version", addresses: "Addresses", dataFolder: "Data folder",
  expiringSoon: "Expiring soon", runningLow: "Running low", nothingYet: "Nothing yet",
  addItem: "Add item", ask: "Ask the assistant", comingSoon: "This part is not built yet.",
  setupTitle: "Set up your household", setupIntro: "Choose a name for this hub and a household password. Everyone who knows the password can pair a phone and change anything.",
  hubName: "Hub name", language: "Language", password: "Household password", passwordAgain: "Repeat password",
  passwordRule: "At least 8 characters. Anyone at this laptop can change it later.", finish: "Finish setup",
  passwordsDiffer: "The passwords do not match.", pairedDevices: "Paired devices", noDevices: "No phones are paired yet.",
  addDevice: "Add a phone", remove: "Remove", cancel: "Cancel", done: "Done",
  pairTitle: "Pair a phone", pairStep1: "1. Scan this code with the phone camera (or open the address below) and install the app:",
  pairStep2: "2. Open Zaklon on the phone, tap \"Scan the QR code\", scan this code and enter the household password.", pairCode: "Or type the code",
  pairExpires: "The code works for 5 minutes.", devicePaired: "Phone paired.", changePassword: "Change password",
  english: "English", serbian: "Serbian", lastSeen: "Last seen", never: "never",
  connectTitle: "Connect to your hub", connectIntro: "On the laptop, open Household and choose \"Add a phone\". Then scan the code it shows.",
  scanQr: "Scan the QR code", findHubs: "Find hubs on this network", noHubsFound: "No hub answered on this network.",
  cameraDenied: "Camera access was not allowed.", notAPairingCode: "That is not a Zaklon pairing code.",
  pairCodeEntry: "Code from the laptop (6 digits)", deviceName: "Name of this phone", deviceNameHint: "e.g. Ana's phone",
  pairNow: "Pair", linkedTo: "Connected to", forgetHub: "Forget this hub", thisDevice: "This phone",
  addonsIntro: "Knowledge packs, maps, AI models and helper programs. Downloads resume after interruptions and are verified before use.",
  diskFree: "Free disk space", of: "of", battery: "Battery", pluggedIn: "charging", batteryRule: "Downloads need at least 50% battery or the charger.",
  catKnowledge: "Knowledge", catMaps: "Maps", catModels: "AI models", catApps: "Programs",
  recommended: "recommended", download: "Download", queued: "Queued", pause: "Pause", resume: "Resume", retry: "Retry",
  installed: "Installed", verifying: "Verifying…", importTitle: "Import from a USB stick or folder",
  importIntro: "Pick the folder that holds the pack files (or its zaklon-packs subfolder). Files are verified before they are used.",
  importBtn: "Import", imported: "Imported", nothingToImport: "No matching pack files were found there.",
};

const sr: typeof en = {
  home: "Početna", library: "Biblioteka", maps: "Mape", supplies: "Zalihe", assistant: "Asistent",
  addons: "Dodaci", household: "Domaćinstvo", more: "Više",
  hubStatus: "Hub", online: "Radi", offline: "Nije dostupan", devices: "Uređaji", uptime: "Radi već",
  version: "Verzija", addresses: "Adrese", dataFolder: "Folder sa podacima",
  expiringSoon: "Ističe uskoro", runningLow: "Ponestaje", nothingYet: "Još ništa",
  addItem: "Dodaj stavku", ask: "Pitaj asistenta", comingSoon: "Ovaj deo još nije napravljen.",
  setupTitle: "Podesi domaćinstvo", setupIntro: "Izaberi ime za ovaj hub i lozinku domaćinstva. Svako ko zna lozinku može da upari telefon i menja šta god treba.",
  hubName: "Ime huba", language: "Jezik", password: "Lozinka domaćinstva", passwordAgain: "Ponovi lozinku",
  passwordRule: "Najmanje 8 znakova. Svako za ovim laptopom može kasnije da je promeni.", finish: "Završi podešavanje",
  passwordsDiffer: "Lozinke se ne poklapaju.", pairedDevices: "Upareni uređaji", noDevices: "Još nijedan telefon nije uparen.",
  addDevice: "Dodaj telefon", remove: "Ukloni", cancel: "Otkaži", done: "Gotovo",
  pairTitle: "Upari telefon", pairStep1: "1. Skeniraj ovaj kod kamerom telefona (ili otvori adresu ispod) i instaliraj aplikaciju:",
  pairStep2: "2. Otvori Zaklon na telefonu, pritisni \"Skeniraj QR kod\", skeniraj ovaj kod i unesi lozinku domaćinstva.", pairCode: "Ili ukucaj kod",
  pairExpires: "Kod važi 5 minuta.", devicePaired: "Telefon je uparen.", changePassword: "Promeni lozinku",
  english: "Engleski", serbian: "Srpski", lastSeen: "Poslednji put", never: "nikad",
  connectTitle: "Poveži se sa hubom", connectIntro: "Na laptopu otvori Domaćinstvo i izaberi \"Dodaj telefon\". Zatim skeniraj kod koji se prikaže.",
  scanQr: "Skeniraj QR kod", findHubs: "Pronađi hubove na ovoj mreži", noHubsFound: "Nijedan hub se nije javio na ovoj mreži.",
  cameraDenied: "Pristup kameri nije dozvoljen.", notAPairingCode: "To nije Zaklon kod za uparivanje.",
  pairCodeEntry: "Kod sa laptopa (6 cifara)", deviceName: "Ime ovog telefona", deviceNameHint: "npr. Anin telefon",
  pairNow: "Upari", linkedTo: "Povezan sa", forgetHub: "Zaboravi ovaj hub", thisDevice: "Ovaj telefon",
  addonsIntro: "Paketi znanja, mape, AI modeli i pomoćni programi. Preuzimanja se nastavljaju posle prekida i proveravaju pre upotrebe.",
  diskFree: "Slobodno na disku", of: "od", battery: "Baterija", pluggedIn: "puni se", batteryRule: "Za preuzimanje treba bar 50% baterije ili punjač.",
  catKnowledge: "Znanje", catMaps: "Mape", catModels: "AI modeli", catApps: "Programi",
  recommended: "preporučeno", download: "Preuzmi", queued: "Na čekanju", pause: "Pauziraj", resume: "Nastavi", retry: "Pokušaj ponovo",
  installed: "Instalirano", verifying: "Provera…", importTitle: "Uvoz sa USB-a ili iz foldera",
  importIntro: "Izaberi folder u kome su fajlovi paketa (ili njegov podfolder zaklon-packs). Fajlovi se proveravaju pre upotrebe.",
  importBtn: "Uvezi", imported: "Uvezeno", nothingToImport: "Tamo nema odgovarajućih fajlova paketa.",
};

const dict: Record<Lang, typeof en> = { en, sr };
export type Key = keyof typeof en;

export function makeT(lang: Lang) {
  return (k: Key): string => dict[lang][k] ?? dict.en[k] ?? k;
}
