import type { Dict } from "../en";

export const common: Dict["common"] = {
  cancel: "Cancelar",
  save: "Guardar",
  edit: "Editar",
  create: "Crear",
  delete: "Eliminar",
  remove: "Quitar",
  close: "Cerrar",
  reopen: "Reabrir",
  none: "Ninguno",
  loading: "Cargando…",
  txn: (n) => `${n} mov.`,
  lines: (n) => `${n} línea${n === 1 ? "" : "s"}`,
  transactions: (n) => `${n} movimiento${n === 1 ? "" : "s"}`,
  months: ["Ene", "Feb", "Mar", "Abr", "May", "Jun", "Jul", "Ago", "Sep", "Oct", "Nov", "Dic"],
  monthsLong: [
    "Enero", "Febrero", "Marzo", "Abril", "Mayo", "Junio",
    "Julio", "Agosto", "Septiembre", "Octubre", "Noviembre", "Diciembre",
  ],
  categoryKind: { income: "ingreso", expense: "gasto", neutral: "neutro" },
  uncategorised: "Sin categoría",
  backendDown: "El motor de la aplicación no arrancó",
  loadingScenario: "Cargando escenario…",
};
