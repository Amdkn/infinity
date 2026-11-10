# aspace-infinity-adapter — D-N051 F8 pas 3 (infinity, l'échelle)

**Adresse :** `fiber:a0:infinity` · **Gel :** `e1a3e4b`
(`e1a3e4b36e5f6143e03a59a7ad86e1abacfd22e8`, fork `Amdkn/infinity` ←
`hydro-project/infinity`, Apache-2.0, dérive 0) · **Version :** 0.1.0 ·
**État visé :** 0 → 2 (adapté + fusionné, §D11).

L'adapter épingle la surface amont au gel sans toucher aucun fichier
amont (H-M2 : le diff du fork est ce dossier seul). Le bind
(`aspace-plugin.json` + `src/population_scheduler.py`) est re-appliqué
**verbatim par Ryan** à l'intégration — pas ici.

## Surface épinglée

| Fonction | Source amont (gel) | Preuve |
|---|---|---|
| Hibernation (`idle` → `shut_down`, réveil sur texte utilisateur seul) | `crates/infinity-daemon/src/session_store.rs` (`SessionEntry`) | `AspaceSessionEntry` + smoke |
| RAP (résultat livré à un agent sans processus) | `crates/infinity-daemon/src/rap_callback.rs` (`serve_callbacks`) | `RapOutbox` + test `rap_callback_flow` amont |
| Proxy MCP (serveurs MCP inchangés) | `crates/infinity-daemon/src/mcp_proxy.rs` | test `mcp_proxy_callback.rs` + vrai serveur `rap-test-servers` |
| Memory / session store | `crates/infinity-daemon/src/memory_store.rs` | `PersistentConversationStore` / `PersistentStateStore` |
| Daemon (`infinity-daemon ready`) | `crates/infinity-daemon/src/lib.rs` | ligne de démarrage, socket Unix, WS 127.0.0.1:8080 |

## Constantes (pas des garanties)

- **Cap `75_000`** — constante reprise du scheduler / README (§D8).
  **Non mesuré, non garanti** : ce n'est pas un test de charge.
- **`fractal_layer = "4L"`** — passé explicitement partout
  (`GoldfishSchedule::new`). Le défaut `8L` du scheduler du bind ne
  s'applique jamais. L'incohérence 4L/8L est cosmétique (F8 D6).

## Réseau — localhost seulement (§D6)

`127.0.0.1` / `::1` / `localhost` / socket Unix. `0.0.0.0` et toute
adresse routable sont refusés par `LocalhostPolicy` avant toute
écoute. Télémétrie zéro par défaut — voir
`infinity-TELEMETRY-AUDIT.md` (évidence `D-N051/p3/`).

## Reprise au checkpoint — CONDITIONNELLE (H-P3-7, §D4)

L'hibernation elle-même est prouvée par le daemon. La **reprise au
checkpoint** utilise les checkpoints de pi (pas 2) : le pas 2 n'est
pas vert, donc cette preuve est consignée comme **conditionnelle**,
pas comme verte. Elle sera re-prouvée quand le pas 2 deviendra vert.

## Lancer

```sh
# Smoke (hibernation + RAP + MCP + localhost + gel + surface)
cargo test --manifest-path aspace-infinity-adapter/Cargo.toml

# Preuves MCP / RAP contre les serveurs réels (rap-test-servers)
cargo test --manifest-path aspace-infinity-adapter/Cargo.toml --test mcp_rap_proof

# Format + lint de l'adapter
cargo fmt --manifest-path aspace-infinity-adapter/Cargo.toml -- --check
cargo clippy --manifest-path aspace-infinity-adapter/Cargo.toml --all-targets -- -D warnings
```

## Hors périmètre

Providers (`bedrock`, `rig`), `slack-bot` : non portés (sauf si
l'audit l'exige, §D9). Aucune modification d'un fichier amont, aucun
re-gel, aucune exposition au-delà de localhost.
