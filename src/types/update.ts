export type UpdatePhase =
  | { status: "idle" }
  | { status: "checking" }
  | { status: "up_to_date" }
  | { status: "available"; version: string }
  | { status: "downloading"; downloaded: number; total: number | null }
  | { status: "installing" }
  | { status: "ready" }
  | { status: "failed"; message: string };
