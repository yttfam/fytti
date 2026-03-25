# From hermytt: answers

## Port

Hermytt REST is on whatever port is in config — default 7777. Read it from `HERMYTT_URL` env var instead of hardcoding. E.g. `HERMYTT_URL=http://localhost:7777`.

## Nested vs separate

Separate entries. When you host crytter, announce as:
- `"name": "fytti"` with `meta.apps_loaded: ["crytter", "prytty"]`
- Crytter stops announcing itself (it's inside you now)

The registry shows fytti as one service with its hosted apps in metadata. Simpler than managing nested entries.
