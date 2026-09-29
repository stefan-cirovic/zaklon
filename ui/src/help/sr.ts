import type { HelpContent } from "./types";

/** The user guide in Serbian (Latin script), with the same topics and sections as en.ts. */
const sr: HelpContent = {
  start: {
    title: "Prvi koraci",
    summary: "Šta je Zaklon, kako laptop i telefoni rade zajedno i čime da počneš.",
    sections: [
      {
        id: "what",
        title: "Šta je Zaklon",
        body: [
          { p: "Zaklon je kućna baza koja radi bez interneta. Na tvom laptopu čuva znanje (na primer Vikipediju i uputstva za prvu pomoć), mape, zalihe u kući i AI asistenta, i radi i onda kad internet ne radi." },
          { p: "Nema naloga, nema oblaka i nema praćenja. Sve ostaje na laptopu i na tvojim telefonima." },
        ],
      },
      {
        id: "hub",
        title: "Laptop je hub",
        body: [
          { p: "Zaklon radi na jednom Windows laptopu (ili stonom računaru). Zovemo ga **hub**: on čuva sve podatke domaćinstva i radi težak posao, kao što su preuzimanja i AI." },
          {
            list: [
              "Zaklon se pokreće sa Windowsom i radi i kad zatvoriš njegov prozor, pa telefoni ostaju povezani. Njegova ikonica je dole desno, pored sata.",
              "Iz menija te ikonice možeš da otvoriš Zaklon, da uključiš ili isključiš **Pokreni sa Windowsom** i da ugasiš Zaklon (**Ugasi Zaklon**).",
              "Program i svi podaci domaćinstva su u jednom folderu koji si izabrao pri instalaciji. Kad se Zaklon deinstalira, podaci ostaju.",
            ],
          },
        ],
      },
      {
        id: "phones",
        title: "Telefoni se povezuju preko WiFi-ja",
        body: [
          { p: "Android telefoni dobijaju aplikaciju Zaklon od samog huba i povezuju se sa njim preko kućne WiFi mreže. Za to ne treba internet, samo ista WiFi mreža." },
          {
            list: [
              "Kod kuće telefon koristi sve što hub ima: zalihe, biblioteku, asistenta i mape.",
              "Van kuće telefon i dalje prikazuje poslednje stanje zaliha, a lista za kupovinu i dalje radi. Vidi [Rad bez interneta](#help/offline).",
              "Kad nema rutera (nestanak struje, vikendica), laptop može da napravi [sopstvenu WiFi mrežu](#help/settings/network).",
            ],
          },
        ],
      },
      {
        id: "first-steps",
        title: "Čime da počneš",
        body: [
          {
            steps: [
              "**Podesi domaćinstvo.** Kad se Zaklon prvi put otvori, izaberi jezik, ime huba i lozinku domaćinstva (najmanje 8 znakova).",
              "**Upari telefone.** Na laptopu izaberi **Dodaj telefon** na [Početnoj](#home), ili otvori [Podešavanja › Uređaji](#settings/devices) i izaberi ga tamo. Vidi [Uparivanje telefona](#help/pairing).",
              "**Preuzmi sadržaj.** U [Dodacima](#addons) osnovni paket jednim dugmetom preuzima znanje, uputstva za prvu pomoć i popravke i AI model koji odgovara ovom računaru. Za to jednom treba internet; posle sve radi i bez njega.",
              "**Unesi zalihe** u [Zalihama](#supplies) i pitaj [Asistenta](#assistant) šta god te zanima.",
            ],
          },
          { note: "Svako ko zna lozinku domaćinstva ima ista prava: može da upari telefon i da menja sve. Izaberi lozinku koju će domaćinstvo zapamtiti i čuvaj je; treba i za šifrovane rezervne kopije." },
          { p: "Prelaziš sa drugog računara? Na ekranu za podešavanje umesto toga vrati kopiju prethodnog huba i telefoni nastavljaju da rade. Vidi [Rezervne kopije](#help/settings/backups)." },
        ],
      },
      {
        id: "finding-your-way",
        title: "Kako da se snađeš",
        body: [
          { p: "Traka na dnu je ista na laptopu i na telefonima:" },
          {
            list: [
              "**Početna**: stanje huba i ono što traži pažnju, na primer zalihe kojima uskoro ističe rok ili kojih ponestaje.",
              "**Asistent**: pitaj šta te zanima, običnim rečima.",
              "**Alati**: svi alati, složeni po temama, na primer [Zalihe](#supplies), [Biblioteka](#library), [Mape](#maps) i [Dodaci](#addons).",
              "**Podešavanja**: telefoni, mreža, rezervne kopije i ostala podešavanja, a na kraju spiska i ova pomoć.",
            ],
          },
          { p: "Na laptopu se na ekranu [Alati](#tools) jedan alat može prikačiti na traku. Tada se pojavljuje u traci na svim uređajima u domaćinstvu." },
          { p: "Svaki ekran na vrhu ima vezu **Kako radi** koja otvara njegovu stranu u ovoj pomoći." },
        ],
      },
      {
        id: "topics",
        title: "Sve je složeno po temama",
        body: [
          { p: "Zaklon slaže alate i vodiče u iste teme na ekranu [Alati](#tools) i u folderima [Dodataka](#addons):" },
          {
            list: [
              "**Zdravlje i prva pomoć**: prva pomoć, lekovi i briga o zdravlju.",
              "**Voda**: kako naći, prečistiti i čuvati vodu za piće.",
              "**Hrana**: recepti, čuvanje hrane, zimnica i konzerviranje.",
              "**Bašta**: gajenje povrća, voća, začinskog bilja i mikrozelenja, i zalivanje.",
              "**Struja**: mali solarni sistemi, baterije i koliko ti struje treba.",
              "**Ugradnja**: postavljanje solara, zalivanja kap po kap, sakupljanja kišnice i pumpi, i uputstva za popravke.",
              "**Znanje**: Vikipedija, knjige i rečnici.",
              "**Mape**: mape svih zemalja.",
            ],
          },
          { p: "Na ekranu Alati svaka tema prikazuje svoje alate i vezu **Vodiči za ovu temu**, koja otvara folder te teme u Dodacima, pa su alati i vodiči za jednu temu na istom mestu. Alat ili vodič koji se tiče više tema nalazi se pod svakom od njih." },
        ],
      },
    ],
  },

  pairing: {
    title: "Uparivanje telefona",
    summary: "Instaliraj aplikaciju na Android telefon i poveži ga sa hubom, QR kodom ili kodom od 6 cifara.",
    open: { href: "#settings/devices", label: "Otvori Podešavanja › Uređaji" },
    sections: [
      {
        id: "before",
        title: "Pre nego što počneš",
        body: [
          {
            list: [
              "Telefon ima Android 9 ili noviji.",
              "Telefon i laptop su na **istoj WiFi mreži**. Preko mobilnog interneta ovo ne radi.",
              "Zaklon radi na laptopu i znaš lozinku domaćinstva.",
            ],
          },
        ],
      },
      {
        id: "laptop",
        title: "Na laptopu",
        body: [
          {
            steps: [
              "Izaberi **Dodaj telefon** pored **Uređaji** na [Početnoj](#home), ili otvori [Podešavanja › Uređaji](#settings/devices) i tamo izaberi **Dodaj telefon**.",
              "Laptop prikazuje dva QR koda, jedan za preuzimanje aplikacije i jedan za uparivanje, i kod od 6 cifara.",
              "Kodovi važe 5 minuta. Kad isteknu, izaberi **Novi kod**.",
            ],
          },
        ],
      },
      {
        id: "install",
        title: "Instaliraj aplikaciju na telefon",
        body: [
          {
            steps: [
              "Skeniraj prvi QR kod kamerom telefona, ili adresu ispod njega upiši u pregledač na telefonu. Izgleda otprilike ovako: **http://192.168.1.10:8480/get**.",
              "Preuzmi aplikaciju i instaliraj je. Android možda traži dozvolu za instaliranje aplikacija iz pregledača; dozvoli je za ovu instalaciju.",
            ],
          },
        ],
      },
      {
        id: "qr",
        title: "Uparivanje QR kodom (najlakše)",
        body: [
          {
            steps: [
              "Otvori Zaklon na telefonu i pritisni **Skeniraj QR kod**.",
              "Skeniraj QR kod za uparivanje sa laptopa.",
              "Unesi lozinku domaćinstva i ime telefona, na primer „Anin telefon“, pa pritisni **Upari**.",
            ],
          },
          { p: "QR kod nosi identitet laptopa, pa telefon zna da razgovara baš sa tvojim hubom." },
        ],
      },
      {
        id: "code",
        title: "Uparivanje preko „Pronađi hubove“ i koda od 6 cifara",
        body: [
          { p: "Ovo koristi kad kamera telefona ne može da skenira kod." },
          {
            steps: [
              "Na telefonu pritisni **Pronađi hubove na ovoj mreži** i sa spiska izaberi svoj hub.",
              "Ukucaj kod od 6 cifara koji piše na laptopu.",
              "Proveri da li je **Sigurnosni kod** na telefonu isti kao onaj na laptopu.",
              "Unesi lozinku domaćinstva i ime telefona, pa pritisni **Upari**.",
            ],
          },
          { note: "Pre nego što pošalje lozinku, telefon pomoću koda od 6 cifara proverava da razgovara sa tvojim laptopom, a ne sa nekim drugim uređajem na mreži. Ako se javi neko drugi, uparivanje se prekida i lozinka se ne šalje." },
        ],
      },
      {
        id: "tries",
        title: "Pogrešan kod ili lozinka",
        body: [
          {
            list: [
              "Jedan kod dozvoljava tri pokušaja. Posle previše pogrešnih pokušaja izaberi **Novi kod** na laptopu.",
              "Telefon koji je previše puta pogrešio mora da sačeka nekoliko minuta pre sledećeg pokušaja.",
              "Lozinka domaćinstva se nikad ne čuva na telefonu.",
            ],
          },
        ],
      },
      {
        id: "paired",
        title: "Upareni telefoni",
        body: [
          {
            list: [
              "[Podešavanja › Uređaji](#settings/devices) prikazuju sve uparene telefone i kad je koji poslednji put viđen.",
              "Na laptopu **Ukloni** trajno isključuje telefon. Sa njim se brišu i njegovi sačuvani razgovori sa asistentom.",
              "Na samom telefonu **Zaboravi ovaj hub** (u Podešavanjima › Uređaji) prekida vezu; da bi ponovo koristio hub, upari ga ponovo.",
              "Ako je laptop ponovo instaliran ili zamenjen, telefon to kaže i nudi **Upari ponovo**. Do tada sve na telefonu ostaje sačuvano.",
            ],
          },
        ],
      },
    ],
  },

  home: {
    title: "Početna",
    summary: "Domaćinstvo na prvi pogled: hub, ono što traži pažnju, asistent, zalihe i dodaci.",
    open: { href: "#home", label: "Otvori Početnu" },
    sections: [
      {
        id: "state",
        title: "Stanje huba",
        body: [
          { p: "Na vrhu je ime huba i da li **Radi** ili **Nije dostupan**. Pored toga:" },
          {
            list: [
              "**Napajanje**: baterija laptopa i da li se puni. Računar bez baterije prikazuje **Na struji**.",
              "**Uređaji**: koliko je telefona upareno. Na laptopu **Dodaj telefon** pored toga odmah prikazuje kodove za uparivanje (vidi [Uparivanje telefona](#help/pairing)).",
              "**Adrese**: gde telefoni na WiFi mreži nalaze hub.",
            ],
          },
        ],
      },
      {
        id: "warnings",
        title: "Upozorenja",
        body: [
          { p: "Ispod stanja huba stoji ono što traži pažnju, samo kad ga ima:" },
          {
            list: [
              "Hub ne radi, ili ga telefon ne vidi.",
              "Windows zaštitni zid ne pušta telefone; dugme to popravlja. Vidi [Mrežu](#settings/network).",
              "Izašla je novija verzija Zaklona.",
            ],
          },
        ],
      },
      {
        id: "ask",
        title: "Pitaj bilo šta",
        body: [
          { p: "Upiši pitanje u **Pitaj bilo šta** i pritisni Enter: otvara se [Asistent](#assistant) i postavlja ga u novom razgovoru. Ispod polja su poslednja tri razgovora ovog uređaja; izaberi neki da ga otvoriš." },
        ],
      },
      {
        id: "lists",
        title: "Zalihe",
        body: [
          { p: "**Zahteva pažnju** je jedan spisak, najhitnije prvo:" },
          {
            list: [
              "ono čemu je **istekao** rok (crveno), i pre koliko;",
              "ono čemu rok **ističe** u narednih 30 dana (narandžasto), prvo ono što ističe najpre;",
              "ono čega **ponestaje**: ima ga manje nego što piše u **Upozori kad padne ispod**, na primer „2 od 3 kg“.",
            ],
          },
          { p: "Prikazuje se do šest redova; ostalo je u [Zalihama](#supplies). Kad ništa ne traži pažnju, kartica samo kaže **Sve je u redu**." },
          { p: "Pored onoga čemu je istekao rok ili čega ponestaje je **Dodaj na listu za kupovinu**; ono što je već na listi ima oznaku **Na listi je**. Ispod spiska, **Za kupovinu** kaže koliko stvari ima na listi za kupovinu i otvara je." },
        ],
      },
      {
        id: "addons",
        title: "Biblioteka i dodaci",
        body: [
          { p: "Koliko je pun disk sa bibliotekom, šta se upravo preuzima i koliko dodataka ima novu verziju, pauzirano je ili nije završeno. Njima upravljaš u [Dodacima](#addons)." },
        ],
      },
      {
        id: "tools",
        title: "Brzi pristup",
        body: [{ p: "Pločica za svaki alat koji nije u traci, i jedna za Pomoć." }],
      },
      {
        id: "away",
        title: "Na telefonu van kuće",
        body: [
          { p: "Kad hub nije dostupan, Početna prikazuje zalihe i razgovore koje je telefon sačuvao, uz vreme od kada su. Vidi [Rad bez interneta](#help/offline)." },
        ],
      },
    ],
  },

  assistant: {
    title: "Asistent",
    summary: "Pitaj običnim rečima. Odgovori stižu iz tvoje biblioteke, sa izvorima, a asistent zna i tvoje zalihe.",
    open: { href: "#assistant", label: "Otvori Asistenta" },
    sections: [
      {
        id: "ask",
        title: "Kako se postavlja pitanje",
        body: [
          {
            steps: [
              "Otvori [Asistenta](#assistant) i upiši pitanje u polje na dnu, na srpskom ili engleskom.",
              "Na laptopu pritisni Enter (Shift+Enter prelazi u novi red). Na telefonu pritisni dugme sa strelicom.",
              "Asistent prvo traži u biblioteci, pa piše odgovor. Kvadratno dugme ga zaustavlja.",
            ],
          },
          { p: "Odgovara na jeziku na kom pitaš. Prvo pitanje posle pauze traje duže jer AI mora da se pokrene (do jednog minuta)." },
        ],
      },
      {
        id: "sources",
        title: "Izvori i citati",
        body: [
          { p: "Asistent odgovara iz paketa znanja u tvojoj [Biblioteci](#library). Brojevi u odgovoru, na primer [1], vode do članaka pod **Izvori**. Pritisni neki da pročitaš članak." },
          {
            list: [
              "Kad u biblioteci nema ničega o tom pitanju, odgovor to kaže. Tada dolazi iz opšteg znanja modela i može biti netačan.",
              "Kad odgovor ne navodi izvore, napomena ispod njega te podseća da ga proveriš u člancima.",
              "Ispod svakog odgovora piše šta je tražio i koliko brzo je pisao.",
            ],
          },
          { warn: "AI može da pogreši. Za sve što je važno, pročitaj članke iz kojih je odgovor." },
        ],
      },
      {
        id: "supplies",
        title: "Pitanja o zalihama",
        body: [
          { p: "Pitaj šta imaš u kući, na primer „Šta ističe ovog meseca?“. Takvi odgovori su označeni sa **Iz tvojih zaliha**." },
          { p: "Možeš da tražiš i izmene, na primer „dodaj 2 litra mleka“ ili „potrošili smo pirinač“. Asistent pokaže šta bi promenio, a ništa se ne menja dok ne izabereš **Da, uradi**." },
        ],
      },
      {
        id: "memory",
        title: "Šta pamti",
        body: [
          { p: "Reci „zapamti da je Ana alergična na penicilin“ i asistent će ponuditi da to zapamti; izaberi **Da, uradi** da se beleška sačuva. Beleške koristi kad su važne za pitanje." },
          {
            list: [
              "Beleške su pod **Šta asistent pamti**, na dnu spiska razgovora, i u [Podešavanjima › AI asistent](#settings/assistant/memory). Tu možeš da ih dodaješ i brišeš.",
              "Svi u domaćinstvu vide iste beleške. Asistent pamti do 500 beleški, svaku do 300 znakova.",
            ],
          },
        ],
      },
      {
        id: "conversations",
        title: "Sačuvani razgovori",
        body: [
          {
            list: [
              "Svaki razgovor se čuva na hubu. Spisak levo (na telefonu dugme sa spiskom na vrhu) ih grupiše po danima i ima pretragu.",
              "**Novi razgovor** počinje nov.",
              "Svaki uređaj vidi samo svoje razgovore: laptop svoje, svaki telefon svoje.",
              "Dugme **Opcije razgovora** (⋯) pored naslova preimenuje ili briše razgovor.",
              "Van dometa huba telefon i dalje može da čita svoj spisak i razgovore koje je poslednje otvarao, ali ne može da pita hub.",
            ],
          },
          { note: "Uređaj čuva do 500 razgovora (mesto oslobađa onaj koji najduže nije korišćen) i do 200 pitanja u jednom razgovoru. Kad se telefon ukloni, brišu se i njegovi razgovori." },
        ],
      },
      {
        id: "send",
        title: "Pošalji na…",
        body: [
          { p: "Da podeliš razgovor, otvori njegove opcije, izaberi **Pošalji na…** i uređaj. Tamo se kopija pojavi kao nov razgovor, označen imenom uređaja koji ga je poslao. Tvoj razgovor ostaje kakav je bio." },
        ],
      },
      {
        id: "online",
        title: "Pretraga na internetu",
        body: [
          { p: "Ispod polja za pitanje je **Traži i na internetu**. Podrazumevano je isključeno. Kad ga uključiš, asistent pretražuje i veb (preko DuckDuckGo-a), samo u tom razgovoru, a stranice koje je pročitao navodi kao izvore sa oznakom **(internet)**. Za to treba internet." },
          { note: "Dok je ovo isključeno, ništa od onoga što pitaš ne izlazi iz kuće." },
        ],
      },
      {
        id: "health",
        title: "Zdravlje i prva pomoć",
        body: [
          { p: "Kod pitanja o zdravlju, povredama i lekovima asistent je posebno oprezan:" },
          {
            list: [
              "Zadržava samo ono što može da potkrepi člankom iz biblioteke.",
              "Ispod odgovora dodaje brojeve za hitne slučajeve: **194** za Hitnu pomoć u Srbiji, **112** u EU.",
              "Kad u biblioteci nema proverenog odgovora, to kaže, navodi te brojeve i savetuje da pitaš lekara ili farmaceuta.",
              "Beleške koje su važne, na primer alergija, prikazuju se iznad odgovora.",
            ],
          },
          { warn: "Zaklon nije lekar. Kad je hitno, prvo pozovi pomoć." },
        ],
      },
      {
        id: "model",
        title: "AI model",
        body: [
          { p: "Asistentu treba AI model, koji se jednom preuzme na laptopu. Kad ga nema, asistent nudi model preporučen za ovaj računar. Veći modeli bolje odgovaraju, ali su sporiji i traže više memorije. Model menjaš u samom Asistentu (polje **Model**) ili u [Podešavanjima › AI asistent](#settings/assistant)." },
          { p: "Model kome treba više memorije nego što ovaj računar ima označen je sa **traži više memorije** i ne može da se izabere: usporio bi ceo računar. Kad nijedan model ne staje, asistent nije dostupan na ovom računaru, a biblioteka, mape, zalihe i telefoni i dalje rade. Kad AI kaže da trenutno nema dovoljno slobodne memorije, zatvori neke programe i pitaj ponovo." },
          { p: "AI se pokreće uz prvo pitanje i sam se gasi posle 20 minuta bez pitanja. Na laptopu možeš da ga ugasiš odmah, vezom **oslobodi memoriju sada**." },
        ],
      },
      {
        id: "phone",
        title: "AI na samom telefonu",
        body: [
          { p: "Kod kuće telefon pita AI na hubu. Telefon može da ima i svoj mali model, za kad hub nije dostupan:" },
          {
            steps: [
              "Na telefonu otvori Asistenta i izaberi **AI na ovom telefonu (bez huba)**.",
              "Pod **Modeli na hubu** izaberi **Kopiraj na telefon**. To se radi jednom, preko WiFi-ja, a ekran ostaje upaljen dok se ne završi.",
              "Izaberi **Pokreni**, pa pitaj.",
            ],
          },
          { p: "Van dometa huba odgovara AI sa telefona. Manji je i ne traži u biblioteci, zato proveri ono što kaže." },
        ],
      },
    ],
  },

  supplies: {
    title: "Zalihe",
    summary: "Šta imaš u kući, gde stoji i kad ističe, sa listom za kupovinu i skeniranjem barkoda.",
    open: { href: "#supplies", label: "Otvori Zalihe" },
    sections: [
      {
        id: "items",
        title: "Stavke",
        body: [
          { p: "Zalihe su zajedničke za celo domaćinstvo: izmena na jednom uređaju vidi se na svima. Kartica **Stavke** prikazuje sve, uz pretragu i izbor kategorije. Da dodaš nešto:" },
          {
            steps: [
              "Izaberi **Dodaj stavku**.",
              "Upiši naziv, količinu i jedinicu, kategoriju i mesto (na primer Ostava ili Frižider). Možeš da dodaš i svoje mesto, na primer „Vikendica“.",
              "Upiši rok trajanja ako ga ima, i **Upozori kad padne ispod** ako želiš da znaš kad ponestaje.",
              "Izaberi **Sačuvaj**.",
            ],
          },
          { p: "Dugmad **−** i **+** troše ili dodaju po jedan. Pritisni stavku da je izmeniš ili obrišeš; obrisana stavka ostaje zabeležena u istoriji." },
        ],
      },
      {
        id: "batches",
        title: "Serije i rokovi trajanja",
        body: [
          { p: "Ista stvar kupljena u različito vreme često ima različit rok. Svaka kupovina je **serija** sa svojom količinom i rokom. Otvori stavku da vidiš njene serije, da ih izmeniš ili dodaš novu." },
          {
            list: [
              "Stavka prikazuje ukupnu količinu i najraniji rok.",
              "Kad trošiš, troši se prvo serija kojoj rok najpre ističe.",
              "Rokovi su označeni crveno kad su istekli, a bojom akcenta kad ističu u narednih 30 dana.",
            ],
          },
        ],
      },
      {
        id: "running-low",
        title: "Ponestaje",
        body: [
          { p: "Stavci postavi **Upozori kad padne ispod**, na primer 2 litra za mleko. Kad ga bude manje, stavka dobija oznaku **Ponestaje**, pojavljuje se na [Početnoj](#home) i sama odlazi na listu za kupovinu, sa količinom koja nedostaje." },
          { p: "Ako je obrišeš sa liste za kupovinu, vratiće se kad je dopuniš i ponovo počne da ponestaje." },
        ],
      },
      {
        id: "shopping",
        title: "Lista za kupovinu",
        body: [
          {
            list: [
              "U **Dodaj na listu…** upiši sve ostalo što ti treba.",
              "U prodavnici pritisni **Kupljeno** za ono što si kupio. To prelazi u **Spremi**.",
              "**Obriši** skida stavku sa liste.",
            ],
          },
          { note: "Lista za kupovinu radi i na telefonu van kuće. Izmene čekaju na telefonu i stižu na hub kad se vratiš." },
        ],
      },
      {
        id: "put-away",
        title: "Spremi",
        body: [
          { p: "Kad se vratiš kući, otvori **Spremi**. Za svaku kupljenu stvar proveri količinu i jedinicu, upiši rok, mesto i kategoriju, pa izaberi **Spremi**. Dodaje se u zalihe kao nova serija, ili kao nova stavka ako je do sada nisi imao." },
        ],
      },
      {
        id: "history",
        title: "Istorija",
        body: [
          { p: "**Istorija** prikazuje svaku promenu: šta je dodato, izmenjeno, potrošeno, dopunjeno ili obrisano, kada i na kom uređaju." },
        ],
      },
      {
        id: "scan",
        title: "Skeniranje barkoda (telefoni)",
        body: [
          {
            steps: [
              "Na telefonu izaberi **Skeniraj barkod** i usmeri kameru na kod. Prvi put dozvoli pristup kameri.",
              "Ako stavka sa tim barkodom postoji, otvoriće se. Ako ne postoji, počinje nova stavka sa već upisanim barkodom; ako je domaćinstvo ranije dalo ime tom barkodu, upisuje se i ime.",
              "U obrascu stavke, **Skeniraj** pored polja **Barkod** dodaje kod toj stavci.",
            ],
          },
          { note: "Skeniranje se radi na samom telefonu. Ne treba mu internet ni Google servisi." },
        ],
      },
    ],
  },

  library: {
    title: "Biblioteka",
    summary: "Čitaj i pretražuj pakete znanja, poput Vikipedije, bez interneta.",
    open: { href: "#library", label: "Otvori Biblioteku" },
    sections: [
      {
        id: "packs",
        title: "Paketi znanja",
        body: [
          { p: "U biblioteci su **paketi znanja**: Vikipedija, rečnik, medicinski članci, uputstva za prvu pomoć, popravke, baštu i još mnogo toga. Svaki paket se jednom preuzme u [Dodacima](#addons) (ili uveze sa USB-a) i posle radi bez interneta." },
          { p: "Dok nema nijednog paketa, Biblioteka to kaže i nudi **Otvori Dodatke**." },
        ],
      },
      {
        id: "read",
        title: "Čitanje",
        body: [
          { p: "**Knjige** su paketi na hubu. Pritisni jednu da otvoriš njenu početnu stranu, pa prati veze kao na veb sajtu. **Nazad** vraća u Biblioteku." },
        ],
      },
      {
        id: "search",
        title: "Pretraga",
        body: [
          { p: "Upiši nešto u **Pretraži biblioteku…** da pretražiš sve pakete odjednom. Svaki rezultat prikazuje deo članka i paket iz kog je." },
        ],
      },
      {
        id: "latin",
        title: "Srpski članci latinicom",
        body: [
          { p: "Srpska Vikipedija je pisana ćirilicom. Da je čitaš latinicom, uključi **Latinica za srpske članke** u [Podešavanjima › Jezik](#settings/language/latin). Važi samo za ovaj uređaj." },
        ],
      },
      {
        id: "assistant",
        title: "Biblioteka i asistent",
        body: [
          { p: "[Asistent](#assistant) pretražuje iste pakete i navodi članke koje je koristio. Što više paketa imaš, na više pitanja može da odgovori." },
          { note: "Telefoni čitaju biblioteku sa huba, pa moraju da budu na kućnoj WiFi mreži." },
        ],
      },
    ],
  },

  maps: {
    title: "Mape",
    summary: "Mape sveta bez interneta: hub ih preuzme jednom, a telefoni ih prikazuju u aplikaciji CoMaps.",
    open: { href: "#maps", label: "Otvori Mape" },
    sections: [
      {
        id: "how",
        title: "Kako radi",
        body: [
          { p: "Mape su podeljene na delove: države, a velike države i na regione. Hub jednom preuzme delove koje izabereš, a telefoni ih dobijaju od huba preko WiFi-ja, bez interneta. Na telefonima mape prikazuje **CoMaps**, besplatna aplikacija za mape koju takođe daje hub." },
        ],
      },
      {
        id: "download",
        title: "Preuzimanje mapa na hub",
        body: [
          {
            steps: [
              "Otvori [Mape](#maps) i potraži državu ili region.",
              "Izaberi **Preuzmi**. Kod velike države otvori njene regione da preuzmeš samo neke.",
              "Preuzimanja se nastavljaju posle prekida. Na laptopu **Ukloni** briše mapu.",
            ],
          },
          { p: "Iste mape su i u folderu **Mape** u [Dodacima](#addons/maps)." },
        ],
      },
      {
        id: "phone",
        title: "Mape na telefonu",
        body: [
          {
            steps: [
              "Instaliraj CoMaps sa huba: na telefonu otvori [Mape](#maps) i pritisni **Instaliraj CoMaps**, ili skeniraj QR kod sa ekrana Mape na laptopu. Aplikacija stiže na hub zajedno sa prvom mapom koju preuzmeš.",
              "U aplikaciji CoMaps otvori Settings (gore desno), pod **Custom Map Server** upiši adresu sa ekrana Mape i pritisni **Save**. Na telefonu je možeš kopirati dugmetom **Kopiraj adresu**.",
              "Preuzimaj mape u CoMaps kao i obično, prvo osnovnu mapu sveta (oko 60 MB). Sada stižu sa huba, i bez interneta.",
            ],
          },
          { note: "Mape preuzete u CoMaps ostaju na telefonu, pa rade i van kuće." },
        ],
      },
    ],
  },

  power: {
    title: "Kalkulator struje",
    summary: "Koliko baterija, solarnih panela i koliki invertor treba malom rezervnom sistemu za ono što treba da radi kad nestane struje.",
    open: { href: "#power", label: "Otvori kalkulator struje" },
    sections: [
      {
        id: "how",
        title: "Kako radi",
        body: [
          { p: "Napravi spisak onoga što treba da radi kad nestane struje: frižider, nekoliko svetala, telefoni, ruter. Kalkulator sabere energiju koju oni troše za dan i izračuna bateriju, solarne panele i invertor za to. Namenjen je malim sistemima koji napajaju deo kuće i radi bez interneta." },
          { p: "Spisak se čuva na hubu, pa svi u domaćinstvu vide isti, na laptopu i na svakom telefonu. Svako iz domaćinstva može da ga menja; izmena se sačuva trenutak pošto je napravljena." },
        ],
      },
      {
        id: "list",
        title: "Napravi spisak",
        body: [
          {
            steps: [
              "Otvori [Kalkulator struje](#power) u Alatima. Dok je spisak prazan, **Počni od tipičnog spiska** upiše frižider, četiri LED sijalice, tri telefona, ruter i laptop.",
              "Izaberi uređaj pod **Uređaj za dodavanje** i pritisni **Dodaj**. Svaki dolazi sa tipičnom, opreznom vrednošću; natpisna pločica tvog uređaja je tačnija, pa promeni **Snaga (po komadu)** kad je znaš.",
              "Podesi **Komada** i **Rad dnevno** u satima. Frižideri i zamrzivači se sami uključuju i isključuju, pa se umesto toga računaju po energiji dnevno.",
              "Za nešto čega nema na spisku izaberi **Sopstveni uređaj** i upiši mu ime, snagu i sate.",
            ],
          },
          { note: "**Napajanje** kaže da li uređaj radi preko invertora (AC: većina uređaja sa utikačem) ili direktno sa baterije (DC: lampa od 12 V ili USB punjač). DC štedi gubitke invertora." },
        ],
      },
      {
        id: "system",
        title: "Dani, baterija i sunce",
        body: [
          {
            list: [
              "**Dana bez struje**: koliko dugo baterija sama mora da drži, i bez sunca. Jedan dan pokriva većinu nestanaka struje; tri dana je uobičajen savet za pripremljenost.",
              "**Vrsta baterije**: LiFePO4 (litijum-gvožđe-fosfat) sme da se isprazni do oko 80–90%, olovne i AGM samo do oko 50%, pa im treba dvostruko veći kapacitet.",
              "**Napon sistema**: 12 V odgovara malim sistemima. Većima je bolje na 24 ili 48 V, gde ista snaga teče kao manja struja, kroz tanje kablove.",
              "**Sunce**: mesto i mesec daju sate punog sunca dnevno, po podacima PVGIS Evropske komisije. Decembar ima najmanje sunca, pa je izabran prvi; kalkulator pokazuje panele potrebne u svakom mesecu.",
            ],
          },
        ],
      },
      {
        id: "results",
        title: "Pročitaj odgovor",
        body: [
          { p: "Odgovor počinje jednom rečenicom, na primer „2 × 100 Ah 12 V LiFePO4 baterije, oko 1.400 W solarnih panela i invertor od 600 W sa čistim sinusom“. Ispod nje su baterija u Ah i Wh, paneli za svakodnevnu potrošnju i za punjenje prazne baterije za jedan sunčan dan, trajna i vršna snaga invertora i energija dnevno." },
          { p: "**Kako se ovo računa** pokazuje svaku formulu sa tvojim brojevima, a **Izvori** navode odakle su tipične vrednosti." },
          { note: "Frižideri, zamrzivači i pumpe za trenutak povuku nekoliko puta više snage kad se motor pokreće. Vršna snaga invertora mora to da pokrije, a kalkulator kaže koliko." },
        ],
      },
      {
        id: "safety",
        title: "Bezbednost",
        body: [
          { warn: "Sve što se povezuje na kućnu instalaciju, pa i invertore za „balkonske“ solarne panele koji se uključuju u utičnicu, mora da poveže ovlašćeni električar: struja vraćena u mrežu može da ubije ljude koji je popravljaju. Mali sistem od 12 V sa sopstvenim utičnicama možeš da napraviš i sam." },
          {
            list: [
              "Litijumskim baterijama treba ispravan BMS (sistem za upravljanje baterijom), osigurač odmah pored baterije i kablovi dovoljno debeli za tu struju. Puni ih samo na temperaturi između 0 °C i 40 °C.",
              "Olovne akumulatore nikad ne puni u zatvorenoj prostoriji: ispuštaju vodonik, koji može da eksplodira.",
              "Agregat radi samo napolju, najmanje 6 m od prozora i vrata, i nikad se ne uključuje u kućnu utičnicu.",
            ],
          },
        ],
      },
      {
        id: "link",
        title: "Otvoreno preko linka",
        body: [
          { p: "Link može da otvori kalkulator sa sopstvenim spiskom, na primer onim iz asistenta. Taj spisak nije sačuvan: izaberi **Sačuvaj kao spisak domaćinstva** da ga zadržiš, ili **Prikaži sačuvani spisak** da se vratiš na spisak domaćinstva." },
        ],
      },
    ],
  },

  water: {
    title: "Kalkulator vode",
    summary: "Koliko vode za piće da čuvaš i kako da je učiniš bezbednom, i koliko vode treba povrtnjaku koji se zaliva kap po kap.",
    open: { href: "#water", label: "Otvori kalkulator vode" },
    sections: [
      {
        id: "drinking",
        title: "Voda za piće u rezervi",
        body: [
          {
            steps: [
              "Otvori [Kalkulator vode](#water) u Alatima. Otvara se na delu **Voda za piće u rezervi**.",
              "Podesi koliko odraslih, dece i ljubimaca pije iz tvoje rezerve, dugmadima **−** i **+** ili upisivanjem broja.",
              "Izaberi za koliko dana: **3 dana**, **1 nedelja**, **2 nedelje**, ili **Drugo** da upišeš bilo koji broj.",
            ],
          },
          { p: "Prva količina je za piće i kuvanje: najmanje 3,8 L (jedan galon) po osobi dnevno, kako savetuje Ready.gov. Druga dodaje pranje i osnovnu higijenu: najmanje 15 L po osobi dnevno, minimum iz humanitarnih standarda Sphere. Ispod su iste količine u kanisterima od 5, 10 i 20 L i u buradima od 200 L." },
          { note: "Deci, dojiljama i bolesnima može trebati više vode, a po velikoj vrućini potreba može da se udvostruči. Vodu koju si sam natočio menjaj na svakih šest meseci." },
        ],
      },
      {
        id: "safe",
        title: "Kako da voda bude bezbedna za piće",
        body: [
          { p: "Kuvanje je najbolji način: neka bistra voda snažno ključa 1 minut, a iznad 1.000 m nadmorske visine 3 minuta, pa je ostavi da se ohladi. Ako ne možeš da je prokuvaš, koristi običnu varikinu bez mirisa sa 5–9% natrijum-hipohlorita: 2 kapi na litar bistre vode, 4 ako je mutna, i sačekaj najmanje 30 minuta." },
          { warn: "Ni kuvanje, ni varikina, ni filteri ne mogu učiniti bezbednom vodu u kojoj ima goriva, hemikalija ili otrova." },
          { note: "Ovi brojevi su od CDC-a i EPA-e, koji ne stoje iza Zaklona." },
        ],
      },
      {
        id: "garden",
        title: "Navodnjavanje kap po kap",
        body: [
          {
            steps: [
              "Izaberi **Navodnjavanje kap po kap**.",
              "Za svaku leju upiši dužinu i širinu u metrima (ili izaberi **Površina** i upiši kvadratne metre) i šta raste u njoj. **Dodaj leju** dodaje još jednu.",
              "U delu **Vreme u tom mesecu** izaberi mesec (za svaki slučaj najtopliji), upiši geografsku širinu ili izaberi grad u blizini, i uobičajenu dnevnu najvišu i noćnu najnižu temperaturu. Kiša nije obavezna.",
              "Izaberi razmak i protok kapaljki koji piše na tvom crevu za kapanje.",
            ],
          },
          { p: "**Šta bašti treba** pokazuje litre dnevno i nedeljno, koliko ima kapaljki, koliko dugo dnevno da puštaš vodu (ujutru i uveče kad je to duže od sata) i posudu u koju staje voda za jedan dan." },
          { note: "Kofa ili bure podignuti oko 1 m mogu sami da teraju vodu kroz creva, ali je pritisak mali, pa kapaljke daju manje nego što piše na njima. Drži čašu ispod jedne kapaljke 10 minuta da vidiš koliko stvarno daje, i po potrebi puštaj vodu duže." },
          { p: "Ako lokalna meteorološka služba daje referentnu evapotranspiraciju (ET₀), upiši je u polje **Tvoja ET₀** i koristiće se umesto procene. **Kako se ovo računa** pokazuje sve formule i odakle su brojevi." },
        ],
      },
      {
        id: "roof",
        title: "Kišnica sa krova",
        body: [
          { p: "Upiši površinu koju krov pokriva, merenu odozgo, i kišu u tom mesecu kod vremena. Kalkulator pokazuje otprilike koliko litara krov skupi (1 mm kiše na 1 m² je 1 litar, a računa se 80%) i za koliko dana je to dovoljno bašti." },
          { warn: "Kišnicu pre pijenja obavezno prokuvaj ili dezinfikuj." },
        ],
      },
      {
        id: "shared",
        title: "Zajedničko za domaćinstvo",
        body: [
          { p: "Ono što upišeš čuva se na hubu, pa svi u domaćinstvu vide iste brojeve, na laptopu i na svakom telefonu. Svako iz domaćinstva može da ih menja; promena se čuva čim je napraviš, a promena sa drugog uređaja pojavi se i ovde. Telefon van kuće pokazuje brojeve koje je poslednje video." },
        ],
      },
      {
        id: "link",
        title: "Otvoreno preko linka",
        body: [
          { p: "Link može da otvori kalkulator sa sopstvenim brojevima, na primer onim iz asistenta; ono što link ne kaže ostaje kako ga domaćinstvo ima. Ti brojevi nisu sačuvani: izaberi **Sačuvaj za domaćinstvo** da ih zadržiš, ili **Prikaži sačuvane brojeve** da se vratiš na brojeve domaćinstva." },
        ],
      },
    ],
  },

  addons: {
    title: "Dodaci",
    summary: "Preuzimanje paketa znanja, AI modela i mapa, i njihovo kopiranje na USB ili sa njega.",
    open: { href: "#addons", label: "Otvori Dodatke" },
    sections: [
      {
        id: "layout",
        title: "Diskovi i folderi",
        body: [
          { p: "Dodaci izgledaju kao „Ovaj računar“ u Windows Exploreru." },
          {
            list: [
              "**Uređaji i diskovi** prikazuju disk na kom Zaklon drži biblioteku (**Zaklon biblioteka**), koliko je pun i koliko zauzimaju dodaci. Otvori ga da vidiš šta je na njemu, od najvećeg. Traka postaje crvena kad je disk skoro pun.",
              "Na laptopu se drugi diskovi, na primer USB, otvaraju sa kopiranjem i uvozom podešenim na taj disk.",
              "U **Folderima** su dodaci po temama, istim kao na ekranu [Alati](#tools): Zdravlje i prva pomoć, Voda, Hrana, Bašta, Struja, Ugradnja, Znanje i Mape, a zatim AI modeli i Programi. Vodič koji se tiče više tema nalazi se u folderu svake od njih.",
              "Polje za pretragu nalazi dodatke i države u svim folderima. Dugmad pored njega menjaju prikaz između pločica i tabele sa detaljima.",
            ],
          },
        ],
      },
      {
        id: "starter",
        title: "Osnovni paket",
        body: [
          { p: "Na laptopu osnovni paket (**Osnovni paket za Srbiju** ili **Osnovni paket (engleski)**, prema jeziku aplikacije) jednim dugmetom, **Preuzmi sve**, preuzima ono što domaćinstvu treba za početak: znanje, uputstva za prvu pomoć i popravke, AI model koji odgovara ovom računaru i, u srpskom paketu, mapu Srbije." },
        ],
      },
      {
        id: "downloads",
        title: "Preuzimanja",
        body: [
          {
            list: [
              "Izaberi **Preuzmi** na dodatku. Preuzima ga hub, i kad si preuzimanje pokrenuo sa telefona.",
              "Preuzimanje može da se pauzira i nastavi. Posle prekida nastavlja od mesta gde je stalo, ne kreće iz početka.",
              "Svaki fajl se proverava pre upotrebe. Oštećen fajl se odbacuje; izaberi **Pokušaj ponovo**.",
              "Za preuzimanje treba bar 50% baterije ili punjač, i dovoljno mesta na disku.",
              "Programi (motori za biblioteku, asistenta i mape) stižu sami, uz ono čemu su potrebni.",
            ],
          },
        ],
      },
      {
        id: "updates",
        title: "Nove verzije i uklanjanje",
        body: [
          { p: "Kad izađe novija verzija instaliranog paketa ili mape, piše **Dostupna je nova verzija** i pojavi se dugme **Ažuriraj**. Na laptopu **Ukloni** briše dodatak, ili nedovršeno preuzimanje, i oslobađa mesto." },
        ],
      },
      {
        id: "usb-copy",
        title: "Kopiranje na USB",
        body: [
          { p: "Da podesiš drugi Zaklon bez interneta, kopiraj dodatke na USB. Na laptopu:" },
          {
            steps: [
              "Priključi USB i otvori ga pod **Uređaji i diskovi**, ili koristi **Kopiraj na USB** na dnu Dodataka.",
              "Izaberi dodatke koje kopiraš. Na USB možeš da staviš i sam Zaklon (instalaciju za Windows i aplikaciju za telefon), za nekoga ko počinje od nule.",
              "Izaberi **Kopiraj** i sačekaj da piše **Kopirano u**.",
            ],
          },
          { note: "Na FAT32 disk ne staju fajlovi od 4 GB i veći. Za velike pakete koristi disk formatiran kao exFAT ili NTFS." },
        ],
      },
      {
        id: "usb-import",
        title: "Uvoz sa USB-a",
        body: [
          {
            steps: [
              "Na laptopu otvori USB pod **Uređaji i diskovi**, ili koristi **Uvoz sa USB-a ili iz foldera**.",
              "Izaberi folder sa fajlovima paketa (ili njegov podfolder **zaklon-packs**) i izaberi **Uvezi**.",
              "Svaki paket prikazuje napredak u svom folderu. Fajlovi se proveravaju pre upotrebe.",
            ],
          },
        ],
      },
      {
        id: "phone",
        title: "Na telefonu",
        body: [
          { p: "Telefon vidi iste dodatke i može da pokrene preuzimanje na hubu. Osnovni paket, uklanjanje i kopiranje na USB su na laptopu." },
        ],
      },
    ],
  },

  settings: {
    title: "Podešavanja",
    summary: "Telefoni, mreža, rezervne kopije, lozinka i ostala podešavanja, kategoriju po kategoriju.",
    open: { href: "#settings", label: "Otvori Podešavanja" },
    sections: [
      {
        id: "find",
        title: "Kako da nađeš podešavanje",
        body: [
          { p: "Na laptopu se [Podešavanja](#settings) otvaraju na kategoriji **Uređaji**: kategorije su nabrojane levo, a izabrana se prikazuje pored njih. Na telefonu je prvo spisak; izaberi kategoriju da je otvoriš, a strelica na vrhu vraća na spisak." },
          { p: "**Pronađi podešavanje**, na vrhu spiska, razume srpske i engleske reči, sa kvačicama i bez njih. **Pomoć**, na kraju spiska, otvara ovo uputstvo. Mreža i Rezervne kopije postoje samo na laptopu." },
        ],
      },
      {
        id: "devices",
        title: "Uređaji",
        body: [
          { p: "Dodaj telefon i vidi uparene telefone, i kad je koji poslednji put viđen; **Dodaj telefon** na [Početnoj](#home) takođe vodi ovde. Kad bi Windows zaštitni zid blokirao telefone, upozorenje se vidi i ovde. Na telefonu je ovde i **Zaboravi ovaj hub**. Vidi [Uparivanje telefona](#help/pairing)." },
        ],
      },
      {
        id: "network",
        title: "Mreža",
        body: [
          {
            list: [
              "**WiFi mreža sa ovog laptopa**: kad nema rutera (nestanak struje, vikendica), laptop može da bude WiFi mreža za telefone u domaćinstvu, pomoću Windowsove mobilne pristupne tačke. Izaberi **Napravi WiFi mrežu**; telefoni se priključuju skeniranjem QR koda ili upisivanjem prikazane lozinke, pa otvaraju Zaklon.",
              "**Windows zaštitni zid**: kad bi Windows blokirao telefone, pojavi se upozorenje sa dugmetom **Dozvoli telefonima pristup**. Windows zatim traži potvrdu administratora računara.",
              "**Označi ovu mrežu kao privatnu**: kad Windows mrežu laptopa smatra javnom (uobičajeno za novu WiFi mrežu na Windowsu 11), telefoni na njoj ne mogu da dođu do huba. Ako je to tvoja kućna mreža, ovo dugme u upozorenju kaže Windowsu da je smatra privatnom; Windows traži potvrdu administratora. Ne radi to na mreži u kafiću ili hotelu.",
              "**Mrežne adrese**: gde telefoni na istoj mreži nalaze laptop.",
            ],
          },
          { note: "Da bi napravio WiFi mrežu, Windowsu treba veza koju deli: kabl, ili WiFi mreža na koju se ranije povezao. Probaj to jednom dok sve radi, da znaš da je spremno." },
        ],
      },
      {
        id: "backups",
        title: "Rezervne kopije",
        body: [
          {
            list: [
              "Zaklon svaki dan sam čuva kopiju podataka domaćinstva (zalihe i njihovu istoriju, sačuvane razgovore i beleške, uparene telefone i podešavanja) i drži poslednjih 7 dana. Biblioteka, mape i AI modeli nisu u kopiji; vraćaju se preuzimanjem ili sa USB-a.",
              "**Napravi kopiju sada**, ili **Sačuvaj kopiju na USB** da jednu čuvaš dalje od laptopa.",
              "**Šifrovanje kopija**: jednom upiši lozinku domaćinstva i svaka nova kopija biće šifrovana njom. Kopija se otvara samo lozinkom koja je važila kad je napravljena.",
            ],
          },
          { warn: "Ne zaboravi lozinku domaćinstva: bez nje ni Zaklon ne može da otvori šifrovanu kopiju." },
          { p: "**Vrati iz fajla sa kopijom**: izaberi fajl i, za šifrovanu kopiju, lozinku domaćinstva iz vremena kad je napravljena. Zaklon proveri kopiju, a ona zamenjuje trenutne podatke kad se Zaklon sledeći put pokrene (**Pokreni Zaklon ponovo**). Današnji podaci se takođe čuvaju kao kopija. Sada upareni telefoni, lozinka domaćinstva i identitet huba ostaju kakvi jesu." },
          { p: "Prelaziš na nov računar? Instaliraj Zaklon na njemu i na ekranu za podešavanje umesto toga vrati kopiju starog huba. Tada i upareni telefoni, lozinka i identitet huba dolaze iz kopije, pa telefoni nastavljaju da rade." },
        ],
      },
      {
        id: "privacy",
        title: "Privatnost i bezbednost",
        body: [
          {
            list: [
              "**Lozinka domaćinstva** (laptop): ovde se menja. Treba za uparivanje telefona i za otvaranje šifrovane kopije. Već upareni telefoni ostaju povezani.",
              "**Privatnost**: sve ostaje na hubu i tvojim telefonima. Zaklon ide na internet samo kad pokreneš nešto što to traži, na primer preuzimanje ili pretragu na internetu, i jednom dnevno da proveri da li postoji nova verzija, što može da se isključi.",
            ],
          },
        ],
      },
      {
        id: "appearance",
        title: "Izgled",
        body: [
          { p: "Izaberi boju akcenta, i potpuno crnu pozadinu koja štedi bateriju na OLED ekranima. Oboje se pamti samo na ovom uređaju." },
        ],
      },
      {
        id: "language",
        title: "Jezik",
        body: [
          { p: "Svaki uređaj bira svoj jezik, srpski ili engleski. **Latinica za srpske članke** prikazuje srpske članke iz biblioteke latinicom, takođe samo na ovom uređaju." },
        ],
      },
      {
        id: "assistant",
        title: "AI asistent",
        body: [
          { p: "Izaberi koji od preuzetih AI modela asistent koristi; onaj koji odgovara ovom računaru označen je kao preporučen. Modeli se preuzimaju na laptopu, u [Dodacima](#addons/models). Ovde je i **Šta asistent pamti**." },
        ],
      },
      {
        id: "updates",
        title: "Ažuriranja",
        body: [
          { p: "Jednom dnevno, kad ima interneta, Zaklon pita GitHub za broj najnovije verzije; ništa o domaćinstvu se ne šalje. **Proveri sada** pita odmah. Kad izađe novija verzija, **Otvori stranicu za preuzimanje** je otvara u pregledaču." },
          { note: "Zaklon nikad sam ne preuzima niti instalira ažuriranje. Dnevna provera se isključuje na laptopu." },
        ],
      },
      {
        id: "about",
        title: "O programu",
        body: [
          { p: "Verzija i licenca (Zaklon je besplatan i otvorenog koda, zauvek), računar huba (procesor, memorija, slobodan prostor i, na laptopu, folder sa podacima) i licence projekata na kojima je Zaklon zasnovan." },
        ],
      },
    ],
  },

  offline: {
    title: "Rad bez interneta",
    summary: "Šta radi bez interneta ili rutera, i na telefonu kad je laptop ugašen ili daleko.",
    sections: [
      {
        id: "no-internet",
        title: "Nema interneta",
        body: [
          { p: "Zaklon je napravljen baš za to. Kod kuće sve radi bez interneta dok je laptop uključen, a telefoni na istoj WiFi mreži: zalihe, biblioteka, mape i asistent." },
          { p: "Internet treba samo za preuzimanje dodataka, za asistentovu pretragu na internetu i za dnevnu proveru nove verzije." },
        ],
      },
      {
        id: "no-router",
        title: "Nema rutera",
        body: [
          { p: "Bez rutera (nestanak struje, vikendica) laptop može da napravi svoju WiFi mrežu: otvori [Podešavanja › Mreža](#settings/network/hotspot) i izaberi **Napravi WiFi mrežu**. Telefoni se priključe na nju, pa otvore Zaklon." },
        ],
      },
      {
        id: "away",
        title: "Telefon van dometa huba",
        body: [
          { p: "Kad je laptop ugašen ili je telefon van kuće, telefon javlja **Hub nije dostupan** i piše od kada su njegovi podaci. I dalje ima:" },
          {
            list: [
              "poslednje stanje zaliha, za čitanje, i spiskove na Početnoj;",
              "listu za kupovinu, koja i dalje radi: dodaj, označi kao kupljeno ili obriši;",
              "spisak sačuvanih razgovora i one koje je poslednje otvarao, za čitanje;",
              "svoj AI, ako je model kopiran na telefon (vidi [AI na samom telefonu](#help/assistant/phone));",
              "mape koje su već preuzete u CoMaps.",
            ],
          },
          { p: "Za ostale izmene, biblioteku i AI na hubu treba hub." },
        ],
      },
      {
        id: "waiting",
        title: "Izmene koje čekaju",
        body: [
          { p: "Izmene liste za kupovinu napravljene van kuće čekaju na telefonu (**Promene koje čekaju hub**) i same se šalju čim telefon sledeći put dođe do huba." },
          { note: "Ako telefon u međuvremenu upariš sa drugim hubom, pitaće te da li da izmene koje čekaju pošalje tom hubu ili da ih odbaci." },
        ],
      },
      {
        id: "laptop-off",
        title: "Kad je laptop ugašen",
        body: [
          { p: "Hub radi dok je laptop uključen i Zaklon pokrenut (ikonica mu je pored sata). Zatvaranje prozora ga ne gasi; **Ugasi Zaklon** u meniju ikonice ga gasi. Zaklon se ponovo pokreće sa Windowsom." },
          { p: "Dok je laptop ugašen ništa se ne gubi: telefoni se usklade čim se laptop ponovo uključi." },
        ],
      },
    ],
  },

  troubleshooting: {
    title: "Rešavanje problema",
    summary: "Telefon ne može da se poveže, asistent je spor, disk je pun i drugi problemi.",
    sections: [
      {
        id: "connect",
        title: "Telefon ne može da se poveže",
        body: [
          {
            steps: [
              "Proveri da je laptop uključen i da Zaklon radi: ikonica mu je pored sata, a [Početna](#home) na laptopu piše **Radi**.",
              "Proveri da je telefon na **istoj WiFi mreži** kao laptop, a ne na mobilnom internetu. Mreža za goste često razdvaja uređaje; koristi glavnu mrežu.",
              "Na laptopu otvori [Početnu](#home) ili [Podešavanja › Uređaji](#settings/devices). Ako upozorava da **telefoni možda ne mogu da se povežu**, izaberi **Dozvoli telefonima pristup** i potvrdi u Windowsu.",
              "Ako Windows mrežu smatra javnom, ne pušta telefone. Ako je to tvoja kućna mreža, u upozorenju izaberi **Označi ovu mrežu kao privatnu** i potvrdi u Windowsu (ili u Windows podešavanjima otvori **Mreža i internet**, izaberi mrežu i za tip mrežnog profila izaberi **Privatno**).",
              "Ako „Pronađi hubove“ ne nađe ništa, upari telefon QR kodom.",
            ],
          },
          { p: "[Podešavanja › Mreža](#settings/network/firewall) na laptopu pokazuju da li Windows zaštitni zid pušta telefone." },
        ],
      },
      {
        id: "pairing",
        title: "Uparivanje ne uspeva",
        body: [
          {
            list: [
              "**Kod je istekao**: izaberi **Novi kod** na laptopu. Kod važi 5 minuta i tri pokušaja.",
              "**Pogrešna lozinka domaćinstva**: proveri je sa onim ko ju je postavio. Menja se na laptopu, u [Podešavanjima › Privatnost i bezbednost](#settings/privacy/password).",
              "**Umesto huba se javio drugi uređaj**: upari telefon skeniranjem QR koda.",
              "**Hub je ponovo instaliran ili zamenjen**: telefon nudi **Upari ponovo**; do tada sve na njemu ostaje sačuvano.",
            ],
          },
        ],
      },
      {
        id: "slow",
        title: "Asistent je spor ili staje",
        body: [
          {
            list: [
              "Prvo pitanje posle pauze pokreće AI, što traje do jednog minuta.",
              "Manji AI model odgovara brže. Izaberi ga u [Podešavanjima › AI asistent](#settings/assistant) ili ga preuzmi u [Dodacima](#addons/models).",
              "Brzina piše ispod svakog odgovora. Ako laptop ima malo slobodne memorije, zatvori druge programe.",
              "Odgovor koji traje predugo se zaustavlja. Pitaj ponovo, ili koristi manji model.",
              "Ako AI stane dok se učitava, računar možda nema dovoljno slobodne memorije: probaj manji model.",
            ],
          },
        ],
      },
      {
        id: "disk",
        title: "Disk je pun",
        body: [
          {
            list: [
              "U [Dodacima](#addons) traka diska sa Zaklon bibliotekom postaje crvena kad je skoro pun. Otvori disk da vidiš šta zauzima najviše mesta.",
              "Na laptopu **Ukloni** pakete, mape i modele koji ti ne trebaju. I pauzirana i nedovršena preuzimanja zauzimaju mesto.",
              "Preuzimanje koje ne staje javlja **Nema dovoljno slobodnog prostora na disku**.",
              "Na telefonu nedovršene kopije AI modela zauzimaju mesto dok ponovo ne kopiraš model ili ih ne odbaciš.",
            ],
          },
        ],
      },
      {
        id: "library",
        title: "Biblioteka se ne pokreće",
        body: [
          { p: "Ako Biblioteka javlja da njen motor ne može da se pokrene, otvori [Dodaci › Programi](#addons/programs), ukloni **Motor biblioteke (Kiwix)** i preuzmi ga ponovo." },
        ],
      },
      {
        id: "download",
        title: "Preuzimanje ne uspeva",
        body: [
          {
            list: [
              "Izaberi **Pokušaj ponovo**: preuzimanje nastavlja od mesta gde je stalo.",
              "Za preuzimanje treba bar 50% baterije ili punjač.",
              "Oštećen fajl se sam odbacuje; ponovno preuzimanje to rešava.",
            ],
          },
        ],
      },
      {
        id: "restart",
        title: "Ponovno pokretanje Zaklona",
        body: [
          { p: "Ako laptop javlja **Zaklon hub ne radi na ovom računaru**, ponovo pokreni Zaklon: u meniju ikonice pored sata izaberi **Ugasi Zaklon**, pa ponovo otvori Zaklon." },
        ],
      },
    ],
  },
};

export default sr;
