// The one defuss runtime of this page, and the typed surface of defuss-shadcn it uses.
//
// VERIFIED: defuss-shadcn 0.9.11 dist/components/core.js bundles defuss-morph 0.2.0 and
// defuss-query 0.2.0, installs them as globalThis.df$ (with df$.shadcn) and throws when df$ already
// exists. Importing defuss-query or defuss-morph as values would bundle a second morph with its own
// event registry, so both are type-only dependencies here and df$ comes from the global.
//
// Components are imported one by one instead of all.js: the page uses a handful of them. VERIFIED:
// this page builds to 161 kB JS + 173 kB CSS; all.js + all.css alone built to 667 kB + 1.08 MB
// (vite build, 2026-11-01). Order matters: core.css and core.js first, then each component's CSS
// and JS. Every asset is bundled by Vite, because the Content-Security-Policy in defuss-tauri.json
// allows only 'self'.
import 'defuss-shadcn/dist/components/core.css';
import 'defuss-shadcn/dist/components/button/button.css';
import 'defuss-shadcn/dist/components/badge/badge.css';
import 'defuss-shadcn/dist/components/input/input.css';
import 'defuss-shadcn/dist/components/textarea/textarea.css';
import 'defuss-shadcn/dist/components/label/label.css';
import 'defuss-shadcn/dist/components/dialog/dialog.css';
import 'defuss-shadcn/dist/components/separator/separator.css';
import 'defuss-shadcn/dist/components/toolbar/toolbar.css';
import 'defuss-shadcn/dist/components/sidebar/sidebar.css';
import 'defuss-shadcn/dist/components/resizer/resizer.css';
import 'defuss-shadcn/dist/components/border-layout/border-layout.css';
import 'defuss-shadcn/dist/components/data-grid/data-grid.css';
import 'defuss-shadcn/dist/components/virtual-list/virtual-list.css';
import 'defuss-shadcn/dist/components/core.js';
import 'defuss-shadcn/dist/components/toolbar/toolbar.js';
import 'defuss-shadcn/dist/components/sidebar/sidebar.js';
import 'defuss-shadcn/dist/components/resizer/resizer.js';
import 'defuss-shadcn/dist/components/border-layout/border-layout.js';
import 'defuss-shadcn/dist/components/data-grid/data-grid.js';
import 'defuss-shadcn/dist/components/virtual-list/virtual-list.js';
import type { DfDollar } from 'defuss-query';

export type Rec = Record<string, unknown>;
type Side = 'north' | 'south' | 'west' | 'east';
type Target = string | HTMLElement;
type GridState = { name: string; config: { selected?: unknown[] } & Rec };

// defuss-shadcn ships no .d.ts, so only the calls this page makes are declared, from each
// component's component-skill.md.
export interface Shadcn {
  dataGrid: {
    setSource(
      t: Target,
      rows: Rec[],
      o?: {
        idField?: string;
        cells?: Record<string, (el: HTMLElement, rec: Rec) => void>;
        query?: Rec;
        persist?: { area?: 'session' | 'local' | 'none' };
      },
    ): void;
    expandAll(t: Target): void;
  };
  virtualList: {
    setData(
      list: HTMLElement,
      count: number,
      render?: (row: HTMLElement, index: number) => void,
    ): void;
  };
  borderLayout: { toggle(t: Target, side: Side): boolean };
}
/** A component element after its runtime initialized it (the State API of defuss-shadcn). */
export interface Stateful extends HTMLElement {
  api: { setState(name: string, config?: Rec): unknown };
  store: { value: GridState; subscribe(fn: (v: GridState) => void): () => void };
}

export const df$ = (globalThis as unknown as { df$: DfDollar & { shadcn: Shadcn } }).df$;
export const ui = df$.shadcn;
