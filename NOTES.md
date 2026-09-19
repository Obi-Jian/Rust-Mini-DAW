1. ---FATTO --- Volume per traccia — lock-free communication, atomics
2. Timeline + playback position — sincronizzazione inter-thread
3. Filtri DSP — imparare il processing del segnale
4. Synth modulari con fundsp — picco del percorso audio
5. Offset traccia + taglio — se vuoi un DAW più completo

IDEE
- Possibilità di modificare frequenza mentre è in riproduzione
- Integrazione con synth HARD
- Aggiunta Dancing Strings


Fixes:
1- Il bottone muted si resetta quando la traccia si interrompe, rimuoviamo tutti  flag muted facciamo più semplicemente in modo che il bottone forzi lo slider per il volume a 0
2- Se aggiungiamo un filtro e selezioniamo il tipo, mentre la traccia è in riproduzione, non va. Dobbiamo stoppare e farla ripartire per applicare. Se aggiungiamo il filtro e solo dopo premiamo play, funziona (nota: per aggiungerlo, bisogna avviarlo almeno un volta prima)
3- Aggiungere tasto rimuovi filtro
4- Sistemare get_interpolated in modo che L(i) venga inflenzato solo da L(y) e non da R(y) e viceversa (serve tenere conto dei channels). NON URGENTE


UI FIXES:
- sezione per selezionare tracce, cliccando si aprono tutti i bottoni di opzione, in ordine:
    1. volume 
    2. mute (stessa riga)
    3. add filter
    4. dopo aver cliccato add filter, le opzioni filtro


FUNDSP
Mono samples can be retrieved with get_mono and filter_mono methods. The get_mono method returns the next sample from a generator that has no inputs and one or two outputs, while the filter_mono method filters the next sample from a node that has one input and one output:

let out_sample = node.get_mono();
let out_sample = node.filter_mono(sample);