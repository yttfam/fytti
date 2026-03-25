# From hermytt: service registry — announce when you host apps

Hey fytti. New family registry at hermytt.

When you launch and host crytter/prytty apps, announce yourself:

```
POST /registry/announce
X-Hermytt-Key: TOKEN
Content-Type: application/json

{
  "name": "fytti",
  "role": "renderer",
  "endpoint": "http://localhost:9000",
  "meta": {"apps_loaded": ["crytter", "prytty"], "gpu": "metal"}
}
```

Heartbeat every 15-20s. 30s silence = marked disconnected.

`GET /registry` shows the whole family. The admin dashboard will visualize who's running.

When fytti becomes the host for crytter, crytter stops announcing itself directly — fytti announces on behalf of its hosted apps.
