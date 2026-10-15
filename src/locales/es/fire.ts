import type { Dict } from "../en";

const s = (n: number) => (n === 1 ? "" : "s");

export const fire: Dict["fire"] = {
  notAdvice: "No es asesoramiento financiero. Es aritmética sobre los números y supuestos que introduces: la rentabilidad, la inflación, los impuestos y el gasto serán distintos. No es una previsión ni una recomendación de comprar, vender o jubilarse. Consulta con un asesor cualificado antes de tomar decisiones.",
  ledeEmpty: "Indica cuánto gastas y cuánto ahorras, y Coin Heaven calcula cuándo trabajar pasa a ser opcional.",
  todaysMoney: "Todo está en dinero de hoy.",
  saved: "Guardado",
  noSnapshot: "Aún no has apuntado nada en Patrimonio, así que esto parte de cero. Añade allí lo que tienes y se rellenará todo.",

  headlineThere: "Ya lo has conseguido",
  headlineWithin: "En menos de un mes",
  headlineAway: (y, m) =>
    `Trabajar será opcional dentro de ${[
      y > 0 ? `${y} año${s(y)}` : null,
      m > 0 ? `${m} mes${m === 1 ? "" : "es"}` : null,
    ]
      .filter(Boolean)
      .join(" y ")}`,
  notOnPath: "Con este plan, aún no",
  atAge: (a) => `Tendrías ${a} años.`,
  range: (lo, hi) => `Entre ${lo} y ${hi} años, según cómo se porten los mercados.`,
  nothingWithin: (h) =>
    `Con estas cifras no llegas en ${h} años: ahorra más, gasta menos o ambas cosas.`,

  have: "Lo que tienes invertido",
  need: "Lo que necesitas",
  rentCoversAll: "El alquiler ya cubre todo lo que gastas: lo que necesitas es cero, y no es casualidad, es aritmética.",
  needNote: (spend, swr) =>
    `Lo justo para sacar ${spend} al año retirando un ${swr} cada año, de forma que no se acabe.`,
  needNoteTax: (gross, tax, swr) =>
    `Lo justo para sacar ${gross} al año (${tax} se van en impuestos) retirando un ${swr} cada año, de forma que no se acabe.`,
  milestoneLabel: (pct) => (pct >= 100 ? "Meta" : pct === 50 ? "La mitad" : `${pct} %`),
  milestoneAt: (y, age) => (age !== null ? `dentro de ${y} años · a los ${age}` : `dentro de ${y} años`),
  reached: "conseguido",

  yourPlan: "Tu plan",
  spendMonthly: "Lo que gastas",
  saveMonthly: "Lo que ahorras",
  perMonth: "al mes",
  fromLedger: (v) => `Usar mi dato real: ${v} al mes`,
  ledgerWindow: (months) =>
    months >= 12
      ? "Tus últimos 12 meses completos de movimientos."
      : `De momento, con ${months} mes${months === 1 ? "" : "es"} de movimientos.`,
  savingsRate: (v) => `Es el ${v} de lo que ingresas.`,
  markets: "Cómo se portan los mercados",
  aboveInflation: "% por encima de la inflación",
  moods: { cautious: "Prudente", balanced: "Equilibrado", hopeful: "Optimista" },
  marketsHint: "Lo que rinden tus inversiones al año una vez descontada la inflación. «Equilibrado» es más o menos lo que ha dado la bolsa a largo plazo.",
  ageNow: "Tu edad hoy",
  addAge: "Añádela y las fechas pasan a ser edades.",

  path: "El camino",
  portfolio: "Tus inversiones",
  target: "Lo que necesitas",
  targetMarker: "meta",
  band: "Rango",
  age: (a) => `${a} años`,
  year: (y) => `Año ${y}`,
  pathNote: "La zona sombreada es el rango entre un escenario prudente y uno optimista. La línea de puntos es plana porque todo va en dinero de hoy: la inflación ya está descontada.",
  pathNotePension: "La línea de puntos baja según se acerca tu pensión: cada año que esperas es un año menos que tus inversiones tienen que aguantar solas. La zona sombreada es el rango entre prudente y optimista.",

  levers: "Qué lo adelantaría",
  leverSave: (v) => `Ahorrar ${v} más al mes`,
  leverSpend: (v) => `Gastar ${v} menos al mes`,
  monthsEarlier: (n) => `${n} mes${n === 1 ? "" : "es"} antes`,
  noChange: "sin cambios",
  tryIt: "Pulsa para probarlo",
  leversNote: "Pulsa una para ver el efecto. Gastar menos cuenta doble: necesitas menos dinero y lo que no gastas se invierte.",

  otherIncome: "Alquileres y pensión",
  sumRent: (v) => `${v} de alquiler al año`,
  sumPension: (v, age) => `${v} de pensión desde los ${age}`,
  sumNone: "sin indicar",
  rent: "Alquiler neto al año",
  rentHint: "Una vez descontados gastos, hipoteca e impuestos.",
  pension: "Pensión al año",
  pensionHint: "En dinero de hoy. Hasta que empiece, tus inversiones cubren todo el gasto solas.",
  pensionAge: "La pensión empieza a los",
  rentBody: (share, residual) =>
    `El alquiler cubre el ${share} de lo que gastas y deja ${residual} al año para que lo cubran tus inversiones. Por eso el valor del inmueble no suma arriba: su aportación ya está aquí, y contarla dos veces adelantaría la fecha en años que no existen.`,
  totalWealth: (v) => `Todo lo que tienes, inmuebles incluidos, suma ${v}.`,

  tax: "Impuestos al retirar dinero",
  sumTax: (v) => `contando ${v} al año`,
  sumTaxOff: "sin contar",
  taxToggle: "Tener en cuenta los impuestos de lo que retire",
  taxHint: "Solo tributa la parte de cada retirada que es ganancia, a los tipos de abajo. Déjalo activado: si no, lo que necesitas sale más corto de lo real.",
  gainShare: "Parte de cada retirada que es ganancia %",
  gainShareHint: "Aproximadamente la mitad, en un fondo que llevas muchos años.",
  fromPortfolio: (v) => `Usar mi cartera: el ${v} es ganancia`,

  careful: "Cuánto tiene que durar el dinero",
  sumCareful: (swr, band) => `retirando ${swr} al año · rango ±${band}`,
  swr: "Retirada anual %",
  swrExplain: (swr, withdrawal, target) =>
    `Cuando dejes de trabajar, cada año vivirás de una parte de tus inversiones. Con un ${swr}, sacar ${withdrawal} al año requiere ${target}. Una retirada más baja hace que el dinero dure más, pero necesitas tener más; el 4 % es la regla de siempre para unos treinta años. Pulsa una fila para probar otra.`,
  needs: (v) => `requiere ${v}`,
  bandLabel: "Mercados buenos y malos ± %",
  bandExplain: (lo, hi, early, late) =>
    `Nadie sabe qué harán los mercados, así que el gráfico sombrea un rango alrededor de tu previsión: desde ${lo} al año si van mal hasta ${hi} si van bien. De ahí sale el «entre ${early} y ${late} años».`,
  goalAge: "Me gustaría dejarlo a los",
  goalYes: (age) => `Sí: dejarlo a los ${age} es posible.`,
  goalSpare: (y) => `Llegas ${y} años antes con estas cifras.`,
  goalJust: "Llegas, pero casi sin margen.",
  goalNo: (age) => `A los ${age} todavía no, con estas cifras.`,
  goalGap: (age, have, need) => `A los ${age} tendrías ${have} y necesitarías ${need}.`,
  goalFix: "Cualquiera de estas te lleva",
  goalSave: (v) => `Ahorrar ${v} más al mes`,
  goalSpend: (v) => `Gastar ${v} menos al mes`,
  goalLater: (age) => `O dejarlo a los ${age}`,
  goalNone: "Ningún cambio realista de ahorro o gasto lo consigue. La palanca es una edad más tardía.",
  brackets: "Impuesto sobre las ganancias de inversión",
  bracketsHint: "Elige un país para usar sus tipos y corrige lo que sea distinto en tu caso o haya cambiado. Lo que escribas aquí es tuyo y se guarda con tu plan.",
  taxCountry: "Normas fiscales de",
  taxOwn: "Mis propios tipos",
  taxOption: (name, tax) => `${name} — ${tax}`,
  taxSource: (tax, year, source) =>
    `${tax}${year === null ? "" : `, ${year}`}${source === null ? "" : `. Compruébalos en ${source}`}.`,
  taxOwnNote: "Estos tipos los has puesto tú y no coinciden con los de ningún país que Coin Heaven conozca.",
  taxAddCountry: "¿No está tu país? Escribe sus tipos abajo. Para añadirlo para todo el mundo basta un archivo pequeño: mira regions/README.md en el código.",
  bracketUpTo: "Hasta",
  bracketAbove: "A partir de ahí",
  bracketAll: "Toda la ganancia",
  bracketAdd: "Añadir un tramo",
  bracketRemove: "Quitar este tramo",
  already: "ya has llegado",
  yr: (y) => `${y} años`,
};
