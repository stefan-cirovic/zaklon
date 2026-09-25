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
  pairTitle: "Pair a phone", pairStep1: "On the phone, open this address in the browser and install the app:",
  pairStep2: "Then open Zaklon on the phone, scan this code and enter the household password.", pairCode: "Or type the code",
  pairExpires: "The code works for 5 minutes.", devicePaired: "Phone paired.", changePassword: "Change password",
  english: "English", serbian: "Serbian", lastSeen: "Last seen", never: "never",
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
  pairTitle: "Upari telefon", pairStep1: "Na telefonu otvori ovu adresu u pregledaču i instaliraj aplikaciju:",
  pairStep2: "Zatim otvori Zaklon na telefonu, skeniraj ovaj kod i unesi lozinku domaćinstva.", pairCode: "Ili ukucaj kod",
  pairExpires: "Kod važi 5 minuta.", devicePaired: "Telefon je uparen.", changePassword: "Promeni lozinku",
  english: "Engleski", serbian: "Srpski", lastSeen: "Poslednji put", never: "nikad",
};

const dict: Record<Lang, typeof en> = { en, sr };
export type Key = keyof typeof en;

export function makeT(lang: Lang) {
  return (k: Key): string => dict[lang][k] ?? dict.en[k] ?? k;
}
