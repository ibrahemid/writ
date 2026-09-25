export class SceneCancelledError extends Error {
  constructor() {
    super("the scene was cancelled");
    this.name = "SceneCancelledError";
  }
}

export class SceneTimeoutError extends Error {
  constructor(
    readonly waitingFor: string,
    readonly timeoutMs: number,
  ) {
    super(`waited ${timeoutMs} ms for ${waitingFor}`);
    this.name = "SceneTimeoutError";
  }
}

export class SceneWindowMissingError extends Error {
  constructor() {
    super("no app window is registered");
    this.name = "SceneWindowMissingError";
  }
}

export class SceneNoteMissingError extends Error {
  constructor(readonly note: string) {
    super(`the demo folder has no note at ${note}`);
    this.name = "SceneNoteMissingError";
  }
}

export class SceneLineMissingError extends Error {
  constructor(
    readonly path: string,
    readonly line: number,
  ) {
    super(`${path} has no line ${line}`);
    this.name = "SceneLineMissingError";
  }
}

export class ScenePaletteMissingError extends Error {
  constructor() {
    super("the search palette's input is not on the page");
    this.name = "ScenePaletteMissingError";
  }
}

export class SceneStateError extends Error {
  constructor(
    readonly scene: string,
    detail: string,
  ) {
    super(`${scene}: ${detail}`);
    this.name = "SceneStateError";
  }
}
