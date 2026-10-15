import type { Dict } from "../en";

export const nav: Dict["nav"] = {
  cashflow: "Cash flow",
  wealth: "Patrimonio",
  fire: "FIRE",
  simulator: "Simulador",
  assistant: "Asistente",
  settings: "Ajustes",
  tagline: "Patrimonio e inmuebles",
  phraseUnconfirmed: "Frase de recuperación sin confirmar",
  unlocked: "Cifrado · desbloqueado",
  locked: "Bloqueado",
  version: (v, schema) => `v${v} · esquema ${schema}`,
  toTheme: (dark) => `Cambiar a tema ${dark ? "oscuro" : "claro"}`,
  privacyOn: "Ocultar importes",
  privacyOff: "Mostrar importes",
};
