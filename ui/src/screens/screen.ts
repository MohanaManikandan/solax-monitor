export interface Screen {
  readonly element: HTMLElement;
  show(param: string | null): void;
  hide(): void;
}
