//! The language of text the backend shows to a person.
//!
//! Most text lives in the UI's own dictionaries. What reaches the screen from
//! here is error messages, the importer's notes about a file, and the
//! matcher's reasons for pairing two transactions. Those are written in
//! English at the point they are raised — where the code that knows what went
//! wrong lives — and translated in one place on the way out:
//!
//!   * errors, in `AppError`'s `Serialize`, through [`translate`];
//!   * sentences assembled from fragments, where a whole-sentence template
//!     cannot follow the grammar, inline with [`pick`].
//!
//! A test scans the source for every error message and fails if one has no
//! Spanish, so a new message cannot ship half-translated.

use std::sync::atomic::{AtomicU8, Ordering};

static LANG: AtomicU8 = AtomicU8::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    Es,
}

pub fn set(code: &str) {
    LANG.store(u8::from(code.eq_ignore_ascii_case("es")), Ordering::Relaxed);
}

pub fn current() -> Lang {
    if LANG.load(Ordering::Relaxed) == 1 {
        Lang::Es
    } else {
        Lang::En
    }
}

/// Choose between two phrasings written side by side.
pub fn pick<T>(en: T, es: T) -> T {
    match current() {
        Lang::En => en,
        Lang::Es => es,
    }
}

/// English template → Spanish template. `{}` marks a value, and values keep
/// their order. A value that is itself a known message is translated too, so
/// "field — message" composites come out whole.
const CATALOGUE: &[(&str, &str)] = &[
    // --- backups and storage ---------------------------------------------
    (
        "cannot create backup folder: {}",
        "no se puede crear la carpeta de copias: {}",
    ),
    ("cannot copy {}: {}", "no se puede copiar {}: {}"),
    (
        "the backup folder cannot be the data folder; choose another folder",
        "la carpeta de copias no puede ser la carpeta de datos; elige otra carpeta",
    ),
    (
        "cannot finish the backup file: {}",
        "no se puede terminar el archivo de copia: {}",
    ),
    (
        "cannot list backup folder: {}",
        "no se puede leer la carpeta de copias: {}",
    ),
    (
        "cannot create data dir: {}",
        "no se puede crear la carpeta de datos: {}",
    ),
    ("{} is not a folder", "{} no es una carpeta"),
    (
        "cannot encode path: {}",
        "no se puede codificar la ruta: {}",
    ),
    // --- setup, keys and recovery ----------------------------------------
    (
        "this database has already been set up",
        "esta base de datos ya está configurada",
    ),
    (
        "no legacy key info to upgrade",
        "no hay una clave antigua que actualizar",
    ),
    (
        "no {} found next to the database. Restore it from a backup folder — it is written alongside every backup.",
        "no encuentro {} junto a la base de datos. Recupéralo de una carpeta de copias: se guarda con cada copia.",
    ),
    (
        "legacy key info is invalid: {}",
        "la clave antigua no es válida: {}",
    ),
    (
        "bad Argon2 parameters: {}",
        "parámetros de Argon2 incorrectos: {}",
    ),
    (
        "key derivation failed: {}",
        "no se ha podido derivar la clave: {}",
    ),
    ("key info is missing", "faltan los datos de la clave"),
    (
        "cannot encode the item order: {}",
        "no se puede guardar el orden de los elementos: {}",
    ),
    (
        "that event no longer exists — reload and try again",
        "ese evento ya no existe; vuelve a cargar e inténtalo de nuevo",
    ),
    (
        "this sheet holds far more than a statement can",
        "esta hoja contiene muchísimo más de lo que cabe en un extracto",
    ),
    (
        "cannot read this spreadsheet: {}",
        "no se puede leer esta hoja de cálculo: {}",
    ),
    (
        "this spreadsheet expands to far more than a statement can hold",
        "esta hoja de cálculo se descomprime a mucho más de lo que cabe en un extracto",
    ),
    (
        "that address is not a valid URL",
        "esa dirección no es una URL válida",
    ),
    (
        "a remote address must use https",
        "una dirección remota tiene que usar https",
    ),
    (
        "the address must start with http:// or https://",
        "la dirección tiene que empezar por http:// o https://",
    ),
    (
        "key info is from a version this build cannot read",
        "los datos de la clave son de una versión que esta app no puede leer",
    ),
    (
        "a database already exists here but its key info is missing or unreadable; restore atlas.keyinfo from a backup folder",
        "ya hay una base de datos aquí pero faltan sus datos de clave o no se pueden leer; restaura atlas.keyinfo desde una carpeta de copias",
    ),
    (
        "the reply was too large to read",
        "la respuesta es demasiado grande para leerla",
    ),
    (
        "key info uses unreasonable key-derivation settings",
        "los datos de la clave piden un coste de derivación fuera de lo razonable",
    ),
    (
        "database key must be 64 hex characters",
        "la clave de la base de datos debe tener 64 caracteres hexadecimales",
    ),
    (
        "keys must be 64 hex characters",
        "las claves deben tener 64 caracteres hexadecimales",
    ),
    (
        "{} foreign key violation(s) after migration",
        "{} violación(es) de clave foránea tras la migración",
    ),
    (
        "database mutex poisoned",
        "la base de datos se ha quedado bloqueada; reinicia Coin Heaven",
    ),
    (
        "database already initialised",
        "la base de datos ya está inicializada",
    ),
    (
        "key mutex poisoned",
        "la clave se ha quedado bloqueada; reinicia Coin Heaven",
    ),
    (
        "invalid key-encryption key",
        "la clave de protección no es válida",
    ),
    ("bad nonce length", "longitud de nonce incorrecta"),
    (
        "could not wrap the database key",
        "no se ha podido proteger la clave de la base de datos",
    ),
    (
        "key info salt is invalid: {}",
        "los datos de la clave no son válidos (sal): {}",
    ),
    (
        "key info nonce is invalid: {}",
        "los datos de la clave no son válidos (nonce): {}",
    ),
    (
        "key info ciphertext is invalid: {}",
        "los datos de la clave no son válidos (texto cifrado): {}",
    ),
    (
        "key info nonce has the wrong size",
        "los datos de la clave tienen un tamaño incorrecto",
    ),
    (
        "unwrapped key has the wrong size",
        "la clave tiene un tamaño incorrecto",
    ),
    ("{} is corrupt: {}", "{} está dañado: {}"),
    ("cannot create {}: {}", "no se puede crear {}: {}"),
    (
        "cannot serialise key info: {}",
        "no se han podido guardar los datos de la clave: {}",
    ),
    ("cannot write {}: {}", "no se puede escribir {}: {}"),
    (
        "this database does not use a passphrase",
        "esta base de datos no usa contraseña",
    ),
    (
        "key info has no recovery wrap — it is corrupt",
        "a los datos de la clave les falta la parte de recuperación: están dañados",
    ),
    (
        "cannot generate recovery phrase: {}",
        "no se ha podido generar la frase de recuperación: {}",
    ),
    (
        "a recovery phrase has 12 words; got {}",
        "una frase de recuperación tiene 12 palabras y has escrito {}",
    ),
    (
        "that is not a valid recovery phrase ({}). Check for a mistyped word.",
        "esa frase de recuperación no es válida ({}). Mira si hay alguna palabra mal escrita.",
    ),
    (
        "cannot restrict permissions: {}",
        "no se han podido restringir los permisos: {}",
    ),
    ("cannot remove {}: {}", "no se puede eliminar {}: {}"),
    (
        "a passphrase needs at least {} characters",
        "la contraseña necesita al menos {} caracteres",
    ),
    ("{} is missing", "falta {}"),
    (
        "refusing to rewrap: this key does not belong to this database",
        "no se puede continuar: esta clave no es la de esta base de datos",
    ),
    // --- assistant ---------------------------------------------------------
    ("unknown provider: {}", "proveedor desconocido: {}"),
    (
        "a model name is required",
        "hay que indicar el nombre del modelo",
    ),
    (
        "the assistant is switched off. Turn it on in Settings first.",
        "el asistente está desactivado. Actívalo primero en Ajustes.",
    ),
    (
        "no provider is enabled. Add one in Settings.",
        "no hay ningún proveedor activo. Añade uno en Ajustes.",
    ),
    (
        "{} needs an API key before it can be used",
        "{} necesita una clave de API antes de poder usarse",
    ),
    (
        "nothing is waiting for review — there is nothing to categorise",
        "no hay nada pendiente de revisar, así que no hay nada que categorizar",
    ),
    ("ask something first", "pregunta algo primero"),
    ("{} returned {}: {}", "{} devolvió {}: {}"),
    ("unreadable response: {}", "respuesta ilegible: {}"),
    (
        "the response contained no text",
        "la respuesta no contenía texto",
    ),
    ("cannot read {}: {}", "no se puede leer {}: {}"),
    (
        "the model did not return a usable layout; adjust the columns by hand",
        "el modelo no ha dado una estructura que sirva; ajusta las columnas a mano",
    ),
    // --- categorising, budgets, ledger -----------------------------------
    (
        "that category no longer exists — reload and try again",
        "esa categoría ya no existe; recarga e inténtalo de nuevo",
    ),
    (
        "a budget period is a year (2026) or a month (2026-03), not {}",
        "el periodo de un presupuesto es un año (2026) o un mes (2026-03), no {}",
    ),
    ("{} is not a real period", "{} no es un periodo válido"),
    (
        "a budget is a plan to spend or earn, so it cannot be negative",
        "un presupuesto es lo que piensas gastar o ingresar, así que no puede ser negativo",
    ),
    (
        "an amount of zero is not a transaction — delete it instead",
        "un importe de cero no es un movimiento; elimínalo mejor",
    ),
    (
        "that account already has this exact transaction, or no longer exists",
        "esa cuenta ya tiene este mismo movimiento o ya no existe",
    ),
    (
        "the search engine has to be off, duckduckgo, exa or searxng",
        "el buscador tiene que ser off, duckduckgo, exa o searxng",
    ),
    (
        "a plan month looks like 2026-03, not {}",
        "un mes de plan tiene el formato 2026-03, no {}",
    ),
    (
        "a month's plan can hold at most {} boxes",
        "el plan de un mes admite como máximo {} cajas",
    ),
    (
        "a box is income or expense, not {}",
        "una caja es un ingreso o un gasto, no {}",
    ),
    (
        "a box holds an amount of zero or more; whether it is income or spending is its type",
        "una caja lleva un importe de cero o más; si es ingreso o gasto lo decide su tipo",
    ),
    ("that account no longer exists", "esa cuenta ya no existe"),
    (
        "sample data can only be added to an empty book",
        "los datos de ejemplo solo se pueden cargar en un Coin Heaven vacío",
    ),
    (
        "update checks are not set up for this build",
        "esta versión no tiene configurada la búsqueda de actualizaciones",
    ),
    (
        "the release page did not say which version is latest",
        "la página de versiones no dice cuál es la última",
    ),
    (
        "could not reach the release page: {}",
        "no se ha podido abrir la página de versiones: {}",
    ),
    (
        "the release page answered {}",
        "la página de versiones ha respondido {}",
    ),
    (
        "the release page did not return JSON",
        "la página de versiones no ha devuelto JSON",
    ),
    (
        "a category needs a name",
        "una categoría necesita un nombre",
    ),
    (
        "unknown category kind: {}",
        "tipo de categoría desconocido: {}",
    ),
    (
        "{} must be YYYY-MM-DD",
        "{} debe tener el formato AAAA-MM-DD",
    ),
    (
        "date must be YYYY-MM-DD",
        "la fecha debe tener el formato AAAA-MM-DD",
    ),
    ("an event needs a name", "un evento necesita un nombre"),
    (
        "a transaction cannot reimburse itself",
        "un movimiento no puede reembolsarse a sí mismo",
    ),
    (
        "a transaction cannot be linked to itself",
        "un movimiento no puede vincularse a sí mismo",
    ),
    ("a description is required", "hace falta una descripción"),
    ("start date", "fecha de inicio"),
    ("end date", "fecha de fin"),
    ("from", "desde"),
    ("to", "hasta"),
    // --- import ------------------------------------------------------------
    ("an account needs a name", "una cuenta necesita un nombre"),
    ("unsupported currency: {}", "moneda no admitida: {}"),
    ("a mapping needs a name", "un formato necesita un nombre"),
    (
        "choose the column that holds the signed amount",
        "elige la columna que contiene el importe con signo",
    ),
    (
        "a date format is required, e.g. %d/%m/%Y",
        "hace falta un formato de fecha, p. ej. %d/%m/%Y",
    ),
    ("cannot open {}: {}", "no se puede abrir {}: {}"),
    (
        "{} is {} MB; the limit is {} MB",
        "{} ocupa {} MB; el límite es {} MB",
    ),
    (
        "no rows could be read with this mapping — check the preview",
        "no se pudo leer ninguna fila con este formato: revisa la vista previa",
    ),
    (
        "unknown match kind: {}",
        "tipo de coincidencia desconocido: {}",
    ),
    (
        "a rule needs a name and a pattern",
        "una regla necesita un nombre y un patrón",
    ),
    (
        "that regular expression is not valid: {}",
        "esa expresión regular no vale: {}",
    ),
    (
        "a rule has to do something: set a category, an event, or mark a transfer",
        "una regla tiene que hacer algo: asignar una categoría o un evento, o marcar un traspaso",
    ),
    ("the file is empty", "el archivo está vacío"),
    (
        "cannot read spreadsheet: {}",
        "no se puede leer la hoja de cálculo: {}",
    ),
    (
        "the workbook has no readable sheet",
        "el archivo no tiene ninguna hoja que se pueda leer",
    ),
    // --- scenarios, settings, categories ---------------------------------
    (
        "scenario name is required",
        "el escenario necesita un nombre",
    ),
    (
        "cannot serialise scenario: {}",
        "no se puede guardar el escenario: {}",
    ),
    (
        "stored scenario is unreadable: {}",
        "no se puede leer el escenario guardado: {}",
    ),
    (
        "at least one module has to stay visible",
        "al menos un módulo tiene que seguir visible",
    ),
    (
        "a category cannot be reassigned to itself",
        "una categoría no se puede reasignar a sí misma",
    ),
    (
        "the replacement category does not exist",
        "la categoría de destino no existe",
    ),
    (
        "{} item(s) still use this category — choose a replacement first",
        "{} elemento(s) usan todavía esta categoría; elige antes una de destino",
    ),
    ("unsupported language: {}", "idioma no admitido: {}"),
    // --- net worth and FIRE ----------------------------------------------
    (
        "an asset class needs a name",
        "una clase de activo necesita un nombre",
    ),
    (
        "unknown asset class kind: {}",
        "tipo de clase de activo desconocido: {}",
    ),
    (
        "that asset class no longer exists",
        "esa clase de activo ya no existe",
    ),
    (
        "a class cannot be reassigned to itself",
        "una clase no se puede reasignar a sí misma",
    ),
    (
        "the replacement class does not exist",
        "la clase de destino no existe",
    ),
    (
        "{} snapshot line(s) still use this class — choose a replacement first",
        "{} línea(s) de balances usan todavía esta clase; elige antes una de destino",
    ),
    ("debt cannot be negative", "la deuda no puede ser negativa"),
    ("unknown asset class: {}", "clase de activo desconocida: {}"),
    ("{} — {}", "{} — {}"),
    (
        "safe withdrawal rate must be between 0% and 20%",
        "la retirada segura tiene que estar entre el 0 % y el 20 %",
    ),
    (
        "spending target cannot be negative",
        "el objetivo de gasto no puede ser negativo",
    ),
    (
        "annual savings cannot be negative — spending more than you earn is a different question than this page answers",
        "el ahorro anual no puede ser negativo: gastar más de lo que ingresas es otra pregunta, y no la que responde esta página",
    ),
    (
        "a real return outside −10% to 20% is not a plan",
        "una rentabilidad real fuera del −10 % al 20 % no es realista",
    ),
    (
        "project between 1 and 60 years",
        "proyecta entre 1 y 60 años",
    ),
    (
        "the target age has to be in the future",
        "la edad objetivo tiene que ser mayor que la actual",
    ),
    (
        "the gain share of a withdrawal is between 0% and 100%",
        "la parte de ganancia de una retirada está entre el 0 % y el 100 %",
    ),
    (
        "a pension cannot be negative",
        "una pensión no puede ser negativa",
    ),
    (
        "the band is between 0% and 10% either side",
        "el rango va del 0 % al 10 % a cada lado",
    ),
    (
        "a tax band has a rate between 0% and 100% and a limit that is not negative",
        "un tramo lleva un tipo entre el 0 % y el 100 % y un límite que no sea negativo",
    ),
    ("taxBrackets", "tramos de impuestos"),
    ("gainShareBps", "parte de ganancia"),
    ("annualPension", "pensión anual"),
    ("bandBps", "banda"),
    // --- portfolio -----------------------------------------------------------
    ("a holding needs a name", "una inversión necesita un nombre"),
    (
        "unknown holding movement: {}",
        "movimiento de inversión desconocido: {}",
    ),
    (
        "amounts and units cannot be negative",
        "los importes y las unidades no pueden ser negativos",
    ),
    (
        "a reconciliation needs the number of units held",
        "un ajuste necesita las unidades que tienes",
    ),
    (
        "a schedule cannot end before it starts",
        "una compra periódica no puede terminar antes de empezar",
    ),
    (
        "a recurring buy needs an amount",
        "una compra periódica necesita un importe",
    ),
    (
        "a recurring buy repeats every so many days or on a day of the month — one of the two",
        "una compra periódica se repite cada cierto número de días o un día concreto del mes; elige una de las dos",
    ),
    (
        "a looked-up price cannot be zero",
        "un precio obtenido de internet no puede ser cero",
    ),
    (
        "a price cannot be negative",
        "un precio no puede ser negativo",
    ),
    (
        "unknown price source: {}",
        "origen de precio desconocido: {}",
    ),
    ("an item needs a name", "una partida necesita un nombre"),
    ("date", "la fecha"),
    (
        "{} cannot search the web on its own. Turn on a search engine (DuckDuckGo, Exa or your own SearXNG) in Settings, or enter prices by hand",
        "{} no puede buscar en internet por sí solo. Activa un buscador (DuckDuckGo, Exa o tu propio SearXNG) en Ajustes o escribe los precios a mano",
    ),
    ("Exa did not answer: {}", "Exa no ha respondido: {}"),
    (
        "the model did not return prices Coin Heaven could read. It said: {}",
        "el modelo no ha dado precios que Coin Heaven pueda leer. Respondió: {}",
    ),
    (
        "the model sent back an empty answer; it may have run out of room while thinking. Try again, or use a model without a thinking mode",
        "el modelo respondió en blanco; puede que se quedara sin espacio mientras pensaba. Prueba otra vez o usa un modelo sin modo de razonamiento",
    ),
    (
        "Exa did not accept the key. Check it in Settings",
        "Exa no ha aceptado la clave. Revísala en Ajustes",
    ),
    (
        "Exa is out of credits or limiting requests. Check your Exa account",
        "a Exa se le ha acabado el crédito o está limitando las peticiones. Mira tu cuenta de Exa",
    ),
    ("Exa returned {}", "Exa ha devuelto {}"),
    ("Exa did not return JSON", "Exa no ha devuelto JSON"),
    (
        "DuckDuckGo did not answer: {}",
        "DuckDuckGo no ha respondido: {}",
    ),
    (
        "DuckDuckGo returned no results ({}). It may be limiting requests — wait a few minutes, or use your own SearXNG",
        "DuckDuckGo no ha dado resultados ({}). Puede que esté limitando las peticiones: espera unos minutos o usa tu propio SearXNG",
    ),
    (
        "the search engine did not answer: {}",
        "el buscador no ha respondido: {}",
    ),
    (
        "the search engine returned {}. In SearXNG's settings.yml, `search.formats` has to include `json`",
        "el buscador ha devuelto {}. En el settings.yml de SearXNG, `search.formats` tiene que incluir `json`",
    ),
    (
        "the search engine did not return JSON",
        "el buscador no ha devuelto JSON",
    ),
    (
        "the search engine address has to start with http:// or https://",
        "la dirección del buscador tiene que empezar por http:// o https://",
    ),
    (
        "nothing is left for the assistant: every unfiled merchant already has a suggestion",
        "no queda nada para el asistente: todos los comercios sin categoría ya tienen sugerencia",
    ),
    (
        "that message is too long — ask one thing at a time",
        "ese mensaje es demasiado largo; pregunta una cosa cada vez",
    ),
    ("cannot read this PDF: {}", "no se puede leer este PDF: {}"),
    (
        "no transactions could be read from this PDF. If it is a scan, it holds a picture rather than text; download the statement as CSV or Excel instead",
        "no se ha podido leer ningún movimiento de este PDF. Si es un escaneo, lo que tiene es una imagen y no texto; descarga el extracto en CSV o Excel",
    ),
    (
        "there are no holdings to price yet",
        "aún no hay inversiones que valorar",
    ),
    (
        "this provider cannot search the web",
        "este proveedor no puede buscar en internet",
    ),
    (
        "the model did not return usable prices; try again or enter them by hand",
        "el modelo no ha dado precios que sirvan; prueba otra vez o escríbelos a mano",
    ),
    // --- simulator parameters --------------------------------------------
    ("must be between 1 and 50", "debe estar entre 1 y 50"),
    ("must be between 1 and 600", "debe estar entre 1 y 600"),
    (
        "loan-to-value must be between 0% and 100%",
        "la parte que cubre la hipoteca debe estar entre el 0 % y el 100 %",
    ),
    (
        "occupancy must be between 0% and 100%",
        "el tiempo alquilado debe estar entre el 0 % y el 100 %",
    ),
    (
        "must be between 0% and 100%",
        "debe estar entre el 0 % y el 100 %",
    ),
    (
        "gross yield must be between 0% and 50%",
        "la rentabilidad bruta debe estar entre el 0 % y el 50 %",
    ),
    (
        "rate must be between 0% and 50%",
        "el tipo debe estar entre el 0 % y el 50 %",
    ),
    (
        "must be between −20% and 50% a year",
        "debe estar entre el −20 % y el 50 % al año",
    ),
    (
        "a plan holds at most 60 purchases",
        "un plan admite como máximo 60 compras",
    ),
    (
        "a purchase is planned from month 1 on",
        "una compra se puede planear desde el mes 1",
    ),
    (
        "must be between 0% and 30%",
        "debe estar entre el 0 % y el 30 %",
    ),
    ("must be greater than zero", "debe ser mayor que cero"),
    ("cannot be negative", "no puede ser negativo"),
    // --- field names that appear inside composites -----------------------
    ("swrBps", "retirada anual"),
    ("annualSpendTarget", "gasto anual"),
    ("annualSavings", "ahorro anual"),
    ("realReturnBps", "rentabilidad real"),
    ("horizonYears", "horizonte"),
    ("targetAge", "edad objetivo"),
];

/// Match `msg` against a template, returning the values in its placeholders.
fn capture<'a>(template: &str, msg: &'a str) -> Option<Vec<&'a str>> {
    let parts: Vec<&str> = template.split("{}").collect();
    if parts.len() == 1 {
        return (template == msg).then(Vec::new);
    }
    let mut rest = msg.strip_prefix(parts[0])?;
    let mut values = Vec::with_capacity(parts.len() - 1);
    for (i, part) in parts.iter().enumerate().skip(1) {
        let last = i == parts.len() - 1;
        let at = if last {
            if part.is_empty() {
                rest.len()
            } else {
                // The final literal has to close the message, not just appear.
                rest.len()
                    .checked_sub(part.len())
                    .filter(|&at| &rest[at..] == *part)?
            }
        } else if part.is_empty() {
            return None; // two adjacent values cannot be told apart
        } else {
            rest.find(part)?
        };
        values.push(&rest[..at]);
        rest = &rest[at + part.len()..];
    }
    Some(values)
}

fn fill(template: &str, values: &[String]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut it = values.iter();
    let mut parts = template.split("{}").peekable();
    while let Some(p) = parts.next() {
        out.push_str(p);
        if parts.peek().is_some() {
            out.push_str(it.next().map(String::as_str).unwrap_or(""));
        }
    }
    out
}

/// The message in the current language, or unchanged if it is not known.
///
/// Exact matches are tried before templates, so "cannot be negative" is not
/// mistaken for the tail of a longer message.
pub fn translate(msg: &str) -> String {
    if current() == Lang::En {
        return msg.to_string();
    }
    translate_es(msg)
}

fn translate_es(msg: &str) -> String {
    if let Some((_, es)) = CATALOGUE.iter().find(|(en, _)| *en == msg) {
        return (*es).to_string();
    }
    // Most specific first: the template with the most literal text wins.
    let mut best: Option<(usize, &str, Vec<&str>)> = None;
    for (en, es) in CATALOGUE.iter().filter(|(en, _)| en.contains("{}")) {
        if let Some(values) = capture(en, msg) {
            let literal = en.len() - 2 * values.len();
            if best.as_ref().is_none_or(|(l, _, _)| literal > *l) {
                best = Some((literal, es, values));
            }
        }
    }
    match best {
        Some((_, es, values)) => {
            let values: Vec<String> = values.into_iter().map(translate_es).collect();
            fill(es, &values)
        }
        None => msg.to_string(),
    }
}

/* ---------------------------------------------------- starter names -- */

/// The categories created on first run, in both languages.
const SEED_CATEGORIES: &[(&str, &str)] = &[
    ("Salary", "Salario"),
    ("Rental income", "Ingresos por alquiler"),
    ("Other income", "Otros ingresos"),
    ("Housing", "Vivienda"),
    ("Food & groceries", "Alimentación"),
    ("Transport", "Transporte"),
    ("Leisure & travel", "Ocio y viajes"),
    ("Health", "Salud"),
    ("Subscriptions", "Suscripciones"),
    ("Taxes", "Impuestos"),
    ("Uncategorised", "Sin categoría"),
    ("Internal transfer", "Traspaso interno"),
    ("Investment flow", "Movimientos de inversión"),
];

/// The asset classes created on first run: key, English, Spanish.
const SEED_CLASSES: &[(&str, &str, &str)] = &[
    ("cash", "Cash", "Efectivo"),
    ("equities", "Equities", "Renta variable"),
    ("crypto", "Crypto", "Cripto"),
    ("pension", "Pension", "Pensiones"),
    ("real_estate", "Real estate", "Inmuebles"),
    ("other_asset", "Other asset", "Otros activos"),
    ("mortgage_debt", "Mortgage debt", "Hipoteca"),
    ("other_debt", "Other debt", "Otras deudas"),
];

/// Rename the starter categories and asset classes into `lang`.
///
/// Only a row still carrying its original name in the *other* language is
/// touched, so anything the user renamed stays exactly as they wrote it. Both
/// are data rather than interface — ids, not names, are what rules and
/// history point at — so this is a rename, not a translation layer. A name
/// that would collide with one the user already has is left alone.
pub fn rename_seeds(c: &rusqlite::Connection, lang: &str) -> rusqlite::Result<usize> {
    let to_es = lang.eq_ignore_ascii_case("es");
    let mut changed = 0;
    for (en, es) in SEED_CATEGORIES {
        let (from, to) = if to_es { (en, es) } else { (es, en) };
        changed += c.execute(
            "UPDATE OR IGNORE pl_category SET name = ?1 WHERE name = ?2 AND parent_id IS NULL",
            rusqlite::params![to, from],
        )?;
    }
    for (key, en, es) in SEED_CLASSES {
        let (from, to) = if to_es { (en, es) } else { (es, en) };
        changed += c.execute(
            "UPDATE asset_class SET name = ?1 WHERE key = ?2 AND name = ?3",
            rusqlite::params![to, key, from],
        )?;
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_messages_translate() {
        assert_eq!(
            translate_es("a category needs a name"),
            "una categoría necesita un nombre"
        );
    }

    #[test]
    fn values_are_carried_across() {
        assert_eq!(
            translate_es("12 item(s) still use this category — choose a replacement first"),
            "12 elemento(s) usan todavía esta categoría; elige antes una de destino"
        );
        assert_eq!(
            translate_es("statement.csv is 40 MB; the limit is 25 MB"),
            "statement.csv ocupa 40 MB; el límite es 25 MB"
        );
    }

    #[test]
    fn a_composite_translates_both_halves() {
        assert_eq!(
            translate_es("swrBps — safe withdrawal rate must be between 0% and 20%"),
            "retirada anual — la retirada segura tiene que estar entre el 0 % y el 20 %"
        );
    }

    #[test]
    fn the_most_specific_template_wins() {
        // "{} is missing" must not swallow a longer known message.
        assert_eq!(
            translate_es("atlas.keyinfo is missing"),
            "falta atlas.keyinfo"
        );
        assert_eq!(translate_es("cannot be negative"), "no puede ser negativo");
    }

    #[test]
    fn unknown_text_passes_through_untouched() {
        assert_eq!(
            translate_es("something nobody wrote down"),
            "something nobody wrote down"
        );
    }

    /// Every message the backend can raise has Spanish.
    ///
    /// Scans the source rather than a hand-kept list, so adding an error
    /// without a translation fails here instead of shipping in English.
    #[test]
    fn every_error_message_in_the_source_has_spanish() {
        let re = regex::Regex::new(
            r#"(?s)(?:AppError::(?:Invalid|Other|Keystore|Network)|\bbad|message:)\(?\s*(?:"[a-zA-Z]+",\s*)?(?:format!\(\s*)?"((?:[^"\\]|\\.)*)""#,
        )
        .unwrap();
        let placeholder = regex::Regex::new(r"\{[^{}]*\}").unwrap();
        let continuation = regex::Regex::new(r"\\\n\s*").unwrap();

        let mut missing = Vec::new();
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut stack = vec![root];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    if path.file_name().is_some_and(|n| n != "tests") {
                        stack.push(path);
                    }
                    continue;
                }
                if path.extension().is_none_or(|e| e != "rs") || path.ends_with("tests.rs") {
                    continue;
                }
                let src = std::fs::read_to_string(&path).unwrap();
                let body = src.split("#[cfg(test)]").next().unwrap_or("");
                for m in re.captures_iter(body) {
                    let literal = continuation.replace_all(&m[1], "");
                    let template = placeholder.replace_all(&literal, "{}").to_string();
                    // "{}" on its own wraps another error that is already
                    // covered where it was raised.
                    if template == "{}" {
                        continue;
                    }
                    if !CATALOGUE.iter().any(|(en, _)| *en == template) {
                        missing.push(format!("{}: {template}", path.display()));
                    }
                }
            }
        }
        assert!(
            missing.is_empty(),
            "no Spanish for:\n{}",
            missing.join("\n")
        );
    }

    fn db() -> (tempfile::TempDir, crate::db::Db) {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::db::Db::open(
            &dir.path().join("t.db"),
            &secrecy::SecretString::from("0".repeat(64)),
        )
        .unwrap();
        (dir, db)
    }

    fn names(db: &crate::db::Db, sql: &str) -> Vec<String> {
        db.with(|c| {
            let mut st = c.prepare(sql)?;
            Ok(st
                .query_map([], |r| r.get(0))?
                .collect::<Result<Vec<_>, _>>()?)
        })
        .unwrap()
    }

    #[test]
    fn starter_names_follow_the_language_and_come_back() {
        let (_d, db) = db();
        db.with(|c| Ok(rename_seeds(c, "es")?)).unwrap();
        let cats = names(&db, "SELECT name FROM pl_category ORDER BY sort_order");
        assert_eq!(cats[0], "Salario");
        assert!(cats.contains(&"Alimentación".to_string()));
        assert_eq!(
            names(&db, "SELECT name FROM asset_class WHERE key = 'equities'")[0],
            "Renta variable"
        );

        db.with(|c| Ok(rename_seeds(c, "en")?)).unwrap();
        assert_eq!(
            names(&db, "SELECT name FROM pl_category ORDER BY sort_order")[0],
            "Salary"
        );
    }

    #[test]
    fn a_name_the_user_chose_is_never_touched() {
        let (_d, db) = db();
        db.with(|c| {
            c.execute(
                "UPDATE pl_category SET name = 'Nómina' WHERE name = 'Salary'",
                [],
            )?;
            c.execute(
                "UPDATE asset_class SET name = 'Indexados' WHERE key = 'equities'",
                [],
            )?;
            Ok(rename_seeds(c, "es")?)
        })
        .unwrap();
        let cats = names(&db, "SELECT name FROM pl_category ORDER BY sort_order");
        assert_eq!(cats[0], "Nómina");
        assert_eq!(
            names(&db, "SELECT name FROM asset_class WHERE key = 'equities'")[0],
            "Indexados"
        );
    }

    #[test]
    fn a_collision_with_an_existing_category_is_left_alone() {
        let (_d, db) = db();
        db.with(|c| {
            c.execute(
                "INSERT INTO pl_category (name, kind, sort_order) VALUES ('Vivienda', 'expense', 5)",
                [],
            )?;
            Ok(rename_seeds(c, "es")?)
        })
        .unwrap();
        let cats = names(&db, "SELECT name FROM pl_category");
        assert!(
            cats.contains(&"Housing".to_string()),
            "kept rather than merged"
        );
        assert_eq!(cats.iter().filter(|n| *n == "Vivienda").count(), 1);
    }
}
