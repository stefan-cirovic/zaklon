# Assistant model evaluation

Used to pick the default model for each hardware tier. The same ten prompts are given to every candidate model in Serbian (Latin script) and in English. Each answer is rated 1–5 for correctness, clarity and language quality by a native speaker. Speed (tokens per second) is recorded on the reference laptop (i7-1165G7, 12 GB, CPU only).

## Prompts

1. Explain in three sentences what to do first if someone has a deep cut on the forearm.
2. We have 2 kg of rice, 6 cans of beans and 3 litres of oil for four people. Roughly how many days of lunches is that?
3. List five things to check on a car before a long trip.
4. Translate to Serbian: "The water is safe to drink after boiling it for one minute."
5. Napiši kratak spisak za kupovinu za nedelju dana za dvoje, samo osnovne namirnice.
6. Kako se čuva brašno da ne dobije moljce?
7. Koja je razlika između izraza "rok upotrebe" i "najbolje upotrebiti do"?
8. Objasni detetu od osam godina zašto se ne sme piti voda iz bare.
9. Summarise this text in one sentence: (paste any 200-word Wikipedia paragraph)
10. Odgovori samo "ne znam" ako nisi siguran: koliko stanovnika ima selo Gornji Milanovac?

## Scoring sheet

| Model | Size | tok/s | Avg SR | Avg EN | Notes |
|---|---|---|---|---|---|
| Qwen3.5-2B Q4 | | | | | |
| Qwen3.5-4B Q4 | | | | | |
| Gemma 4 E4B Q4 | | | | | |
| Qwen3.5-9B Q4 (desktop, GPU) | | | | | |
