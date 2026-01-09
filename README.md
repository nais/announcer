# Nais Announcer

En enkel Rust-app som henter siste publiserte annonseringer fra Nais.io og poster de til Slack.
Ved å hashe tittel og innhold vil den også se om det har blitt gjort endringer, og oppdatere Slack ved behov.

## Lokal utvikling

Standardoppsettet for lokal utvikling bruker Valkey som database:

```shell
docker run --name valkey -d valkey/valkey
```

For å kjøre opp appen trenger den tilgang til Slack, og en testkanal å poste til:

```shell
SLACK_TOKEN=token-goes-here
SLACK_CHANNEL_ID=C082AH36ZTL #test-rss-announcements
```

Kjør opp med `Cargo`:

```
cargo run
```

### Kjøring uten Slack og Redis

Før å teste parsing og kjøring av `/reconcile` lokalt, uten å sette opp Slack eller Redis/Valkey, kan du bruke `DRY_RUN`:

```shell
DRY_RUN=1 cargo run
```

I denne modusen:

- sjekkes ikke `SLACK_TOKEN`, `SLACK_CHANNEL_ID` eller Redis-miljøvariabler ved oppstart
- forsøker appen ikke å koble til Redis
- postes det ikke til Slack – det logges bare hva som ville skjedd

Du kan trigge en kjøring lokalt med for eksempel:

```shell
curl -X POST http://localhost:8080/reconcile
```
