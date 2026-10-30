# Bundle macOS, configurazione e preset MobaXterm

I comandi seguenti vanno eseguiti dal checkout:

```sh
cd /Users/leonardosolari/Developer/warp
```

## Creare una .app utilizzabile dal Finder

Installare una volta il bundler nella versione prevista dal repository:

```sh
./script/install_cargo_bundle
```

Creare un bundle ottimizzato, con gli asset incorporati, le risorse e lo schema impostazioni:

```sh
WARP_SKIP_COMMON_SKILLS_INSTALL=1 ./script/run --release \
  --features terminal_highlighting,release_bundle \
  --dont-open --generate-schema
```

In questo ambiente lo script seleziona il canale OSS. Senza un override di `CARGO_TARGET_DIR`,
il risultato previsto è:

```text
/Users/leonardosolari/Developer/warp/target/release/bundle/osx/WarpOss.app
```

Copiare l'intero `WarpOss.app` in `Applicazioni`, aprirlo dal Finder e conservarlo nel Dock.
Il comando è verificato sui sorgenti degli script; questa guida non implica che il bundle sia
già stato compilato o collaudato come pacchetto autonomo.

`script/run` prepara anche il plist, le risorse su filesystem, l'rpath e la firma locale. Il solo
`cargo bundle` non esegue tutti questi passaggi. `release_bundle` impedisce agli asset RustEmbed
di dipendere dai file del checkout. Per una build di sviluppo più rapida:

```sh
WARP_SKIP_COMMON_SKILLS_INSTALL=1 ./script/run \
  --features terminal_highlighting --dont-open
```

Quest'ultima produce normalmente `target/debug/bundle/osx/WarpOss.app`; per spostare il bundle
su altre macchine usare il percorso release con `release_bundle`.

### Copia su altre macchine

Il comando produce la build nativa del toolchain: normalmente arm64 su Apple Silicon, x86_64 su
Intel. Per coprire entrambi servono due build o un bundle universal. Non è sufficiente rinominare
l'eseguibile o copiare il solo `Contents/MacOS/warp-oss`.

Le impostazioni sono dati del singolo utente e non vengono incorporate nel bundle. Copiare anche
il preset e configurarlo sul Mac destinatario. Per archiviare l'intero bundle:

```sh
ditto -c -k --sequesterRsrc --keepParent \
  target/release/bundle/osx/WarpOss.app \
  /private/tmp/WarpHighlight-macos.zip
```

La firma locale dello script usa un certificato Apple Development disponibile o una firma ad hoc.
Per una distribuzione con il normale flusso Gatekeeper occorrono la propria firma Developer ID,
gli entitlement di distribuzione e la notarizzazione Apple. Usare i propri certificati; lo script
release completo del repository contiene invece il flusso di firma aziendale Warp.

`script/run` imposta `NO_LICENSES=1` per le build di sviluppo. Per preparare anche le attribuzioni
delle dipendenze prima di distribuire, eseguire dalla radice del repository:

```sh
./script/install_cargo_release_deps --no-build-deps
APP_BUNDLE="$PWD/target/release/bundle/osx/WarpOss.app"
SETTINGS_SCHEMA_EXECUTABLE="$APP_BUNDLE/Contents/MacOS/warp-oss" \
  ./script/prepare_bundled_resources "$APP_BUNDLE/Contents/Resources" oss
```

Questo cambia le risorse: applicare la firma finale dopo tale passaggio. La build di sviluppo con
`Debug-Entitlements.plist` non costituisce da sola un pacchetto pronto per la notarizzazione.

Riferimento: [notarizzazione macOS, Apple](https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution).

**Nota sul checkout attuale:** non usare per questa feature
`./script/bundle --channel oss --features terminal_highlighting`: il ramo OSS di
`script/macos/bundle` riassegna la lista `FEATURES` dopo il parsing e perde il flag aggiunto.
Il percorso `script/run` sopra conserva il flag.

## Configurare highlight e intestazioni

Aprire `Settings > Appearance > Terminal highlighting`. La Command Palette offre anche
`Configure terminal highlighting and prompt header colors`.

Per una regola:

1. Abilitare `Text highlighting` e premere `Add highlight rule`.
2. Impostare nome e pattern, per esempio `\b(ERROR|FATAL|PANIC)\b`.
3. Scegliere `Target: Text` per la corrispondenza o `Target: Line` per l'intera riga logica.
4. Impostare foreground e/o background, per esempio `#FF0000` o `ansi:bright_red`.
5. Scegliere la sensibilità alle maiuscole, provare il testo di esempio e premere `Save`.

Le frecce cambiano la priorità: la regola più in alto vince sulle celle in conflitto. `Duplicate`,
`Delete` e i pulsanti `On`/`Off` agiscono sulle singole regole. L'anteprima non modifica le
impostazioni attive prima del salvataggio.

Per le intestazioni:

1. Abilitare `Prompt header colors`.
2. Impostare, ad esempio, background `#102030`, foreground `#FFFFFF` e separator `theme:outline`.
   Lasciare vuoto il foreground per conservare i colori originali del prompt.
3. In Classic/PS1 integrato non serve una regex. Per una shell senza integrazione, inserire un
   pattern che riconosca il suo prefisso all'inizio della riga.
4. Controllare l'anteprima con due comandi e premere `Save`.

Per un prompt del tipo `leo@server:~/app$ `, esempio:

```text
^[\w.-]+@[\w.-]+:[^#$\r\n]*[#$][ ]
```

Per un prompt zsh del tipo `leo@Mac ~ % `, esempio:

```text
^[\w.-]+@[\w.-]+[ ][^%\r\n]*%[ ]
```

Sono esempi da adattare al proprio prompt. `[ ]` indica lo spazio finale. Nelle sessioni senza
integrazione un output che imita il prompt può ricevere la stessa decorazione.

I colori accettano `#RRGGBB` o i 16 nomi `ansi:*`. Le intestazioni accettano anche
`theme:surface_2`, `theme:foreground`, `theme:outline`, `theme:accent`. La v1 non ha attributi
configurabili bold, italic, underline o blink e non opera nell'alternate screen.

## Applicare il preset MobaXterm

Il file pronto è [mobaxterm-standard-keywords.toml](presets/mobaxterm-standard-keywords.toml).
Le impostazioni grezze recuperate sono in
[mobaxterm-26.6-extracted.json](presets/mobaxterm-26.6-extracted.json).

Aprire `Open settings file` in Settings e integrare la sezione del preset. Con il canale OSS su
questo Mac il percorso normalmente è `/Users/leonardosolari/.warp-oss/settings.toml`; con il
canale Local è `/Users/leonardosolari/.warp-local/settings.toml`. Il comando UI seleziona il file
corretto anche se si usa un profilo differente.

Se esiste già `[terminal.highlights]`, mantenere una sola sezione e sostituire o integrare le
sue `[[terminal.highlights.rules]]`. Non sostituire tutto `settings.toml`: conservare le altre
impostazioni e l'eventuale `[terminal.prompt_header]`. L'ordine nel preset assegna precedenza a
errori, avvisi, esiti positivi, host e informazioni. Si possono inserire gli stessi campi dalla UI.

| Gruppo | Esempi | Foreground |
| --- | --- | --- |
| Errori | `ERROR`, `permission denied`, `not ok`, `false` | `#FF0000` |
| Avvisi | `WARNING`, `out of memory`, `(WW)` | `#FFFF00` |
| Esiti positivi | `SUCCESS`, `connected`, `OK`, `true` | `#00FF00` |
| Host e valori nulli | `192.168.1.10`, `localhost`, `null`, `none` | `#FF00FF` |
| Informazioni | `INFO`, `loading`, `last login:`, `(II)` | `#00FFFF` |

Prova dopo il caricamento:

```sh
printf '%s\n' \
  'ERROR: permission denied' \
  'WARNING: out of memory' \
  'SUCCESS: connected OK' \
  'localhost 192.168.1.10 null none' \
  'INFO: loading configuration'
```

### Fedeltà del porting

Lo ZIP allegato contiene solo l'eseguibile e due plugin, senza `MobaXterm.ini`. Il riferimento
recuperato staticamente dall'eseguibile 26.6 è il modello incorporato
`Custom: OK/warning/error keywords`; non è una copia delle preferenze personali dell'utente.

Le regex originali sono associate tramite istruzioni x86 ai campi `sEdit1`–`sEdit8` del form
`TFORMSYNTAX`; i nomi e i colori nominali delle categorie sono nelle risorse Delphi del form.
Nel modello recuperato Blue e Blinking non hanno pattern; Underline contiene il pattern URL.
La variante Warp contiene i cinque gruppi con un colore del testo.

Le regex sono state adattate al motore Rust: confini di parola senza consumare i separatori,
supporto all'inizio/fine riga e riconoscimento autonomo dei booleani `true/false/yes/no/ok/ko`.
Queste scelte possono differire sui composti con trattini e sui booleani fuori dai contesti
delimitati originali. I colori RGB seguono i campioni dell'editor MobaXterm; il tema terminale
effettivo di MobaXterm non è stato osservato in esecuzione. Tema e correzione del contrasto di Warp
possono modificare la resa finale.

Il preset non riproduce l'underline URL e il blink, che la v1 non espone. Per queste ragioni è un
adattamento dei gruppi principali, non una promessa di identità visiva o semantica completa.

Verifica del file: parsing TOML e UUID validi; compilazione con la stessa libreria `regex` del
workspace e limite di 1 MiB; 25 controlli di corrispondenza/non corrispondenza passati.

Provenienza statica:

- ZIP SHA-256: `03c827c3467518de45542acba8caae63d951aede4b479962bf065b4df0d8a845`.
- EXE SHA-256: `e9a66144b3a1c4d32f8062b1eb34e26e9ed5298106abf1982b4654ab107f13a5`.
- REA archivio: `ev_c93be5ed35032e900e12391777946cb6021b5d9497f04e3be9d78fbaa61fcc71`.
- REA risorse PE: `ev_df1fa10fba5eb3dba5af94cd6107a4846e942560403c1efeca52e3dd124eb7c3`.
- Decodifica aggiuntiva: Python per stringhe/risorse Delphi, LLVM per le istruzioni x86 statiche.
  Il decompilatore REA non era disponibile; non è stato eseguito codice Windows.

Le evidenze REA originali e il disassemblato LLVM della prova locale sono conservati in
`/private/tmp/mobaxterm-26.6-*-evidence.json` e
`/private/tmp/mobaxterm-standard-keywords-load.txt`.
