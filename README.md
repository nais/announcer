# Nais Announcements

En enkel Rust-app som henter siste publiserte annonseringer fra Nais.io og poster de til Slack.
Ved å hashe tittel og innhold vil den også se om det har blitt gjort endringer, og oppdatere Slack ved behov.

## Lokal utvikling

Den bruker Valkey som database:

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
