# Attivazione e verifica locale

La feature è disponibile nell'app desktop dietro `FeatureFlag::TerminalHighlighting`.
Avvio dal checkout:

```sh
cargo run --features terminal_highlighting
```

In `Settings > Appearance > Terminal highlighting` si possono gestire le regole ordinate e i
colori delle intestazioni. La Command Palette offre i toggle indipendenti per testo e prompt e
l'azione `Configure terminal highlighting and prompt header colors`.

Per sessioni senza integrazione, configurare una regex che riconosca il prefisso del proprio
prompt. Per esempio, `^highlight> ` riconosce un prompt letterale `highlight> `, incluso lo spazio.
Le intestazioni Classic/PS1 integrate usano i marker esistenti e non richiedono una regex.

Le preferenze sono locali. Con il backend `settings.toml` abilitato, esempio minimo:

```toml
[terminal.highlights]
enabled = true

[[terminal.highlights.rules]]
id = "c6c530bc-247f-40a7-9b17-9b149d1b6794"
name = "Errors"
enabled = true
pattern = '\b(ERROR|FATAL|PANIC)\b'
case_sensitive = false
target = "text"
foreground = "ansi:bright_red"

[terminal.prompt_header]
enabled = true
background = "#102030"
foreground = "#FFFFFF"
show_separator = true
separator = "theme:outline"
fallback_regex = '^highlight> '
```

Il contenuto delle griglie e i dati ANSI non vengono riscritti. Ricerca, filtro, selezione,
oscuramento dei segreti e cursore conservano i propri stili. Il matcher opera in background sul
viewport e riusa risultati immutabili; il paint risolve soltanto coordinate e colori.

## Copertura della verifica

I test sono in `crates/warp_terminal/src/model/highlight_tests.rs`,
`app/src/terminal/highlight/*_tests.rs`, `app/src/settings_view/highlighting/mod_tests.rs`,
`app/src/terminal/grid_renderer_tests.rs` e
`crates/integration/src/test/terminal_highlighting.rs`.

I test GUI usano output di una shell reale e verificano i risultati preparati dal renderer,
senza invocare manualmente il matching nelle asserzioni. Il caso senza integrazione avvia
`bash --noprofile --norc -i` senza installare hook e controlla due prompt successivi nella stessa
griglia. Il caso Classic controlla la precedenza dei metadati senza regex e preserva il foreground
del comando. Un ulteriore test usa click e tastiera sull'editor: il salvataggio di una regex
invalida conserva le regole attive e la correzione salva una nuova regola con ID distinto.
Gli screenshot comprendono prompt attivo, storico, elenco delle regole e anteprima in Settings.

Per ripetere i test GUI con screenshot locali:

```sh
WARPUI_USE_REAL_DISPLAY_IN_INTEGRATION_TESTS=1 \
WARP_INTEGRATION_TEST_ARTIFACTS_DIR=/private/tmp/warp-highlighting-artifacts \
cargo nextest run -p integration --test integration -j 1 \
  -E 'test(terminal_highlighting) | test(prompt_headers)' --offline
```

Verifica locale completata su macOS con Rust 1.93:

| Controllo | Esito |
| --- | --- |
| `cargo nextest run -p warp_terminal --offline` | 571 passati, 2 ignorati |
| Test mirati app, controller, renderer, schema e ricerca Settings | 13 passati |
| Test GUI con display reale | 5 passati; screenshot ispezionati |
| Clippy dei quattro package modificati, tutti i target e i test | Passato con l'eccezione descritta sotto |

Clippy con `-D warnings` incontra due occorrenze preesistenti di
`clippy::unnecessary_unwrap` in `crates/warpui_core/src/elements/gui/hoverable.rs` con Rust 1.93.
Il controllo completo dei package modificati passa escludendo soltanto quel lint:

```sh
cargo clippy -p warp_features -p warp_terminal -p warp -p integration \
  --all-targets --tests --features warp/terminal_highlighting --offline -- \
  -D warnings -A clippy::unnecessary_unwrap
```

Gli artefatti delle prove locali sono sotto `/private/tmp/warp-highlighting-artifacts/`.

## Limiti del rilascio

Il flag non è abilitato di default nei canali Dogfood, Preview o Stable. Prima del rollout restano
da effettuare le misure release di latenza/throughput descritte in `TECH.md`, la verifica visuale
su Linux e Windows e una prova con un server SSH reale. La subshell senza hook verifica il
percorso di griglia usato anche dall'output SSH legacy, ma non verifica il trasporto SSH.
La scoperta dei runner cloud non è disponibile localmente: `oz-dev` non è installato.

Le regex del prompt possono riconoscere anche output che imita il prompt. Non vengono inferiti
confini semantici di comando. Alternate screen e front-end TUI restano esclusi dalla v1.
