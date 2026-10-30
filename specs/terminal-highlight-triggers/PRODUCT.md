# Highlight tramite trigger e intestazioni del prompt

Stato: implementazione locale dietro feature flag. Data: 28 ottobre 2026.
Attivazione e verifiche: [IMPLEMENTATION.md](IMPLEMENTATION.md).

## Obiettivo

Rendere immediatamente riconoscibili messaggi importanti e confini fra comandi nel terminale di
Warp. La feature comprende due comportamenti configurabili e indipendenti:

1. Regole regex che evidenziano una corrispondenza oppure la riga che la contiene.
2. Un'intestazione del prompt con colori personalizzabili, soprattutto nelle sessioni senza
   integrazione Warp, per distinguere l'output precedente dall'inizio del comando successivo.

La priorità richiesta è supportare sia sessioni warpified sia sessioni senza integrazione, comprese
quelle SSH non warpified. Il supporto alla presentazione Classic/PS1 con integrazione riutilizza i
confini del prompt già conosciuti da Warp.

## Riferimento iTerm2

La documentazione di iTerm2 distingue le azioni `Highlight Text` e `Highlight Line`, attivate da
espressioni regolari. La proposta riprende queste due interazioni. La sintassi regex di Warp sarà
quella Rust già disponibile nel progetto, con differenze rispetto a ICU usato da iTerm2. La prima
versione non comprende altre azioni del sistema di trigger di iTerm2.

Fonte: [documentazione ufficiale dei trigger](https://iterm2.com/documentation-triggers.html).

## Ambito della prima versione

La destinazione è l'app desktop, su macOS, Linux e Windows. Il matcher deve rimanere indipendente
dalla UI per consentire un successivo utilizzo nel TUI.

| Contesto | Highlight regex | Intestazione personalizzata |
| --- | --- | --- |
| Sessione warpified, locale o remota | Sì | In Classic/PS1, usando i metadati disponibili |
| Sessione senza integrazione, anche SSH | Sì, senza installare componenti remoti | Sì, tramite regex del prompt configurata dall'utente |
| Interfaccia Classic/PS1 con integrazione | Sì | Sì, con gli stessi colori e riconoscimento tramite metadati |
| Programma in alternate screen, ad esempio vim o htop | Escluso dalla prima versione | Escluso dalla prima versione |
| Front-end headless TUI | Estensione successiva | Estensione successiva |

“Sessione senza integrazione” e “interfaccia Classic” sono due condizioni diverse. Il primo caso
deve funzionare anche se l'impostazione globale dell'input è Warp/Universal.

## Regole di highlight

Ogni regola contiene un nome, un pattern regex, un interruttore di attivazione, la sensibilità alle
maiuscole, il target `Text` o `Line` e almeno un colore fra testo e sfondo. I colori possono essere
scelti dalla palette ANSI del tema oppure impostati come RGB personalizzato.

L'utente può aggiungere, modificare, duplicare, eliminare e riordinare le regole. La prima regola
nell'elenco ha la priorità più alta. Per ogni cella vince la prima regola applicabile: un colore
omesso conserva quello di base, senza ereditarlo da una regola successiva.

Esempi di regole aggiungibili, mai attivate automaticamente:

| Nome | Pattern | Target | Esempio di stile |
| --- | --- | --- | --- |
| Errori | `\b(ERROR\|FATAL\|PANIC)\b` | Text | Testo rosso acceso |
| Avvisi | `\bWARN(ING)?\b` | Line | Sfondo ambrato, testo chiaro |
| Completato | `\b(SUCCESS\|PASSED)\b` | Text | Testo verde |

Le regole operano sul testo delle griglie terminali dopo l'interpretazione delle sequenze di
controllo. L'output conserva gli stessi byte e gli stessi attributi ANSI sottostanti; copia,
esportazione testuale e dati inviati alle funzioni AI non acquisiscono colori o marcatori aggiunti.

Nelle sessioni integrate vengono analizzate le griglie di output. Senza integrazione viene
analizzato il flusso visualizzato nella griglia normale: può contenere anche prompt e comandi
riecheggiati, perché il terminale non dispone necessariamente della loro classificazione.
L'editor dell'input, le risposte AI e gli editor di file non ricevono questi highlight.

## Comportamento del matching

- Si considera una riga logica: il soft wrap non interrompe il pattern; un newline reale sì.
- Si evidenziano tutte le corrispondenze non vuote e non sovrapposte di ogni regex. Le
  sovrapposizioni fra regole si risolvono con l'ordine dell'elenco.
- `Line` colora tutte le righe visive della riga logica corrispondente. Lo sfondo copre la larghezza
  della griglia, compresa la parte vuota a destra.
- L'output incompleto viene rivalutato mentre arriva, senza attendere un newline. Se una riscrittura
  elimina una corrispondenza, viene eliminato anche il relativo highlight.
- Modifiche alle regole si applicano anche allo storico ancora disponibile, valutato quando diventa
  visibile. Non occorre rieseguire un comando.
- Resize, scroll, filtro dell'output e cambio tema mantengono le corrispondenze associate al testo
  corretto. Una regola disabilitata rimuove subito il proprio effetto.
- Ricerca, filtro, link in hover, selezione, cursore e redazione dei segreti conservano la propria
  precedenza visiva. Un highlight non può rendere visibile un segreto oscurato.

## Intestazione del prompt nelle sessioni senza integrazione

L'utente abilita `Prompt header colors` e configura:

- Colore dello sfondo dell'intestazione.
- Colore del testo del prompt, opzionale: se omesso restano i colori ANSI del prompt.
- Visibilità e colore di un separatore sottile sopra l'intestazione.
- Una regex di riconoscimento del prompt, con anteprima sul testo di esempio.

La regex deve riconoscere un prefisso non vuoto all'inizio della riga logica. Il testo corrispondente
è considerato prompt. Ad esempio, per prompt del tipo `leo@server:~/app$ `:

```text
^[\w.-]+@[\w.-]+:[^#$\r\n]*[#$][ ]
```

Lo spazio finale è rappresentato da `[ ]`. L'esempio è adattabile al proprio prompt; non è una
regex universale per tutte le shell. La UI deve rendere visibili gli spazi finali nell'anteprima.

Quando il pattern corrisponde, Warp colora lo sfondo della riga logica che contiene il prompt,
inclusi gli eventuali soft wrap e il comando riecheggiato sulla stessa riga. Il colore del testo
personalizzato si applica solo al prefisso riconosciuto. Il separatore viene disegnato sopra la
prima riga visiva. Il resto dell'output mantiene lo stile ordinario e gli highlight configurati.

La decorazione resta nello storico fintanto che il testo riconosciuto è conservato. Non introduce
righe, caratteri o spazi nel terminale. Una modifica della regex rivaluta lo storico visibile.

La regex configurata viene valutata sulle griglie di output normale anche quando la shell esterna
è integrata: un SSH non warpified o una subshell senza hook possono essere contenuti nell'output
di un comando locale. Non basta controllare l'integrazione della pane per escludere questi casi.

La prima versione riconosce una riga logica del prompt: per prompt con più righe separate da newline
l'utente può scegliere la riga che deve fungere da intestazione. Il riconoscimento di un intero
prompt multilinea senza metadati resta un'estensione successiva.

Non si presume che `$`, `#` o `>` identifichino automaticamente un prompt. La regex non è
precompilata né abilitata per conto dell'utente. Testo di output indistinguibile dal pattern può
essere colorato come intestazione: l'anteprima e la descrizione devono rendere esplicito questo
limite. Il riconoscimento è solamente visivo e non crea blocchi, exit code o eventi di esecuzione.

## Intestazione in Classic/PS1 con integrazione

Quando Warp possiede metadati validi del prompt, usa quei confini e non la regex. Gli stessi colori
si applicano al prompt attivo e alle intestazioni storiche PS1. Lo sfondo copre l'area del prompt e
del comando; il foreground personalizzato modifica soltanto il prompt, inclusi RPROMPT e le sue
righe note. Il testo del comando conserva il proprio highlighting.

Il passaggio fra sessioni integrate e non integrate mantiene le preferenze e sceglie la sorgente di
riconoscimento disponibile. Il riconoscimento tramite regex non deve dipendere da `honor_ps1`, da
un remote server o dal completamento della warpification. Le intestazioni native Warp nella
presentazione Universal conservano il comportamento corrente.

## Impostazioni e anteprima

Inserire in `Settings > Appearance` controlli ricercabili per `Text highlighting` e
`Prompt header colors`. La gestione delle regole può aprire un editor dedicato con lista ordinata e
anteprima. L'editor del prompt mostra almeno due comandi con relativo output, così da valutare la
separazione fra un comando e il successivo.

Gli errori del pattern compaiono accanto al campo; un salvataggio non valido non sostituisce la
configurazione attiva. L'anteprima usa lo stesso matcher e gli stessi limiti del terminale. La
modifica del draft aggiorna solo l'anteprima fino a `Save`; `Cancel` scarta il draft.

Aggiungere le azioni Command Palette per aprire la configurazione e abilitare/disabilitare
separatamente highlight e intestazioni del prompt. Non occorrono scorciatoie predefinite.

Le preferenze sono locali e modificabili anche in `settings.toml`; non richiedono un account né
sincronizzazione cloud. Default: nessuna regola, highlight pronto ad applicare le regole aggiunte,
intestazioni personalizzate disabilitate e nessuna regex del prompt. Entrambi gli effetti si possono
disabilitare senza perdere la configurazione.

## Criteri di accettazione

1. La stessa regola evidenzia un messaggio in una sessione warpified e in una sessione senza
   integrazione, anche via SSH non warpified.
2. `ER` e successivamente `ROR` producono lo stesso highlight di `ERROR` ricevuto insieme; una
   riscrittura con `\r` e cancellazione rimuove uno stile divenuto obsoleto.
3. Testo Unicode, soft wrap, resize, scrollback e filtro non spostano l'highlight su altre celle.
4. Due prompt compatibili con la regex, separati dall'output dei comandi, mostrano due intestazioni
   distinguibili anche nello storico, senza abilitare l'integrazione Warp.
5. Un prompt che non corrisponde alla regex non riceve l'intestazione; non si indovina una fine
   comando o un prompt in assenza di evidenza.
6. Classic/PS1 con integrazione usa i marker, supporta prompt multilinea e conserva i colori dopo
   invio del comando, ridimensionamento e cambio tema.
7. Testo copiato, navigazione del cursore, editing e protocollo della sessione restano identici.
8. Regole in conflitto e stile dell'intestazione hanno una precedenza deterministica; ricerca,
   selezione e redazione dei segreti continuano a funzionare.
9. Disabilitare una delle due funzioni rimuove immediatamente il relativo effetto in tutte le pane.
10. Il terminale resta reattivo con molto output; limiti e budget sono definiti nella specifica tecnica.

## Esclusioni ed estensioni successive

La prima versione esclude esecuzione di comandi tramite trigger, notifiche, modifiche a PS1,
importazione dei profili iTerm2, regex fra righe con newline reali, stile dei singoli capture group,
profili per host/comando, alternanza automatica di palette fra comandi e rilevamento semantico dei
confini dei comandi tramite regex. Include invece i due casi principali richiesti: highlight nelle
sessioni integrate e non integrate, e intestazioni colorate nelle sessioni senza integrazione.

Architettura e piano di implementazione: [TECH.md](TECH.md).
