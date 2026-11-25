/**
 * Single source of truth for every icon in the UI.
 *
 * All icons are inline Lucide-style SVG geometry (24x24 viewBox) and are
 * exported as ready-to-use Svelte components. Rendering happens in
 * `IconBase.svelte`, which applies the shared stroke attributes. No icon
 * fonts, emoji, sprites fetched from a CDN, or runtime downloads are used.
 */
import IconBase from './components/IconBase.svelte';

export type Shape =
  | { tag: 'path'; d: string }
  | { tag: 'circle'; cx: number; cy: number; r: number }
  | { tag: 'rect'; x: number; y: number; w: number; h: number; rx?: number }
  | { tag: 'line'; x1: number; y1: number; x2: number; y2: number }
  | { tag: 'polyline'; points: string };

const p = (d: string): Shape => ({ tag: 'path', d });
const c = (cx: number, cy: number, r: number): Shape => ({ tag: 'circle', cx, cy, r });
const r = (x: number, y: number, w: number, h: number, rx = 0): Shape => ({ tag: 'rect', x, y, w, h, rx });
const l = (x1: number, y1: number, x2: number, y2: number): Shape => ({ tag: 'line', x1, y1, x2, y2 });
const pl = (points: string): Shape => ({ tag: 'polyline', points });

export const shapes: Record<string, Shape[]> = {
  plus: [l(12, 5, 12, 19), l(5, 12, 19, 12)],
  paperclip: [
    p('m21.44 11.05-9.19 9.19a6 6 0 0 1-8.49-8.49l9.19-9.19a4 4 0 0 1 5.66 5.66l-9.2 9.19a2 2 0 0 1-2.83-2.83l8.49-8.48'),
  ],
  send: [p('m22 2-7 20-4-9-9-4Z'), p('M22 2 11 13')],
  arrowUp: [l(12, 19, 12, 5), pl('m5 12 7-7 7 7')],
  arrowDown: [l(12, 5, 12, 19), pl('m19 12-7 7-7-7')],
  arrowRight: [l(5, 12, 19, 12), pl('m12 5 7 7-7 7')],
  arrowLeft: [l(19, 12, 5, 12), pl('m12 19-7-7 7-7')],
  folder: [p('M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z')],
  folderOpen: [p('m6 14 1.5-2.5A2 2 0 0 1 9.2 10H20a2 2 0 0 1 2 2v6a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4l2 3h7a2 2 0 0 1 2 2v2')],
  file: [p('M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z'), p('M14 2v5h5')],
  fileText: [p('M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z'), p('M14 2v5h5'), l(8, 13, 16, 13), l(8, 17, 14, 17)],
  fileDiff: [p('M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z'), p('M14 2v5h5'), l(9, 12, 13, 12), l(11, 10, 11, 14), l(15, 17, 16, 17)],
  terminal: [pl('m4 17 6-6-6-6'), l(12, 19, 20, 19)],
  gitBranch: [l(6, 3, 6, 15), c(18, 6, 3), c(6, 18, 3), p('M18 9a9 9 0 0 1-9 9')],
  gitCommit: [c(12, 12, 3), l(1, 12, 9, 12), l(15, 12, 23, 12)],
  plug: [l(12, 22, 12, 17), l(9, 8, 9, 2), l(15, 8, 15, 2), p('M18 8v5a4 4 0 0 1-4 4h-4a4 4 0 0 1-4-4V8Z')],
  sparkles: [
    p('m12 3-1.9 5.8a2 2 0 0 1-1.3 1.3L3 12l5.8 1.9a2 2 0 0 1 1.3 1.3L12 21l1.9-5.8a2 2 0 0 1 1.3-1.3L21 12l-5.8-1.9a2 2 0 0 1-1.3-1.3Z'),
    p('M5 3v3'), p('M3.5 4.5h3'), p('M19 17v3'), p('M17.5 18.5h3'),
  ],
  server: [r(2, 2, 20, 8, 2), r(2, 14, 20, 8, 2), l(6, 6, 6.01, 6), l(6, 18, 6.01, 18)],
  settings: [
    c(12, 12, 3),
    p('M19.4 15a1.7 1.7 0 0 0 .34 1.87l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.7 1.7 0 0 0-1.87-.34 1.7 1.7 0 0 0-1 1.55V21a2 2 0 1 1-4 0v-.09a1.7 1.7 0 0 0-1.11-1.55 1.7 1.7 0 0 0-1.87.34l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06A1.7 1.7 0 0 0 4.6 15a1.7 1.7 0 0 0-1.55-1H3a2 2 0 1 1 0-4h.09A1.7 1.7 0 0 0 4.6 9a1.7 1.7 0 0 0-.34-1.87l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06A1.7 1.7 0 0 0 9 4.6a1.7 1.7 0 0 0 1-1.55V3a2 2 0 1 1 4 0v.09a1.7 1.7 0 0 0 1 1.55 1.7 1.7 0 0 0 1.87-.34l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06A1.7 1.7 0 0 0 19.4 9c.14.61.69 1.05 1.31 1.05H21a2 2 0 1 1 0 4h-.09a1.7 1.7 0 0 0-1.51.95Z'),
  ],
  check: [pl('m20 6-11 11-5-5')],
  checkCircle: [c(12, 12, 9), pl('m8 12 3 3 5-6')],
  x: [l(18, 6, 6, 18), l(6, 6, 18, 18)],
  chevronDown: [pl('m6 9 6 6 6-6')],
  chevronRight: [pl('m9 18 6-6-6-6')],
  chevronLeft: [pl('m15 18-6-6 6-6')],
  chevronUp: [pl('m18 15-6-6-6 6')],
  copy: [r(8, 8, 14, 14, 2), p('M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2')],
  undo: [l(6, 7, 3, 10), l(3, 10, 6, 13), p('M3 10h11a7 7 0 0 1 0 14h-2')],
  redo: [l(18, 7, 21, 10), l(21, 10, 18, 13), p('M21 10H10a7 7 0 0 0 0 14h2')],
  search: [c(11, 11, 8), l(21, 21, 16.65, 16.65)],
  menu: [l(4, 6, 20, 6), l(4, 12, 20, 12), l(4, 18, 20, 18)],
  battery: [r(2, 7, 16, 10, 2), l(22, 11, 22, 13), l(6, 12, 14, 12)],
  upload: [l(12, 16, 12, 4), pl('m7 9 5-5 5 5'), p('M20 16v3a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2v-3')],
  download: [l(12, 4, 12, 16), pl('m7 11 5 5 5-5'), p('M20 16v3a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2v-3')],
  trash: [l(3, 6, 21, 6), p('M8 6V4a1 1 0 0 1 1-1h6a1 1 0 0 1 1 1v2'), p('M19 6l-1 14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2L5 6'), l(10, 11, 10, 17), l(14, 11, 14, 17)],
  stop: [r(6, 6, 12, 12, 2)],
  refresh: [p('M21 12a9 9 0 1 1-2.65-6.36'), pl('M21 3v6h-6')],
  sun: [c(12, 12, 4), l(12, 2, 12, 4), l(12, 20, 12, 22), l(4.93, 4.93, 6.34, 6.34), l(17.07, 17.07, 13.66, 13.66), l(2, 12, 4, 12), l(20, 12, 22, 12), l(4.93, 19.07, 6.34, 13.66), l(17.07, 6.93, 13.66, 10.34)],
  moon: [p('M21 12.8A9 9 0 1 1 11.2 3a7 7 0 0 0 9.8 9.8Z')],
  message: [p('M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2Z')],
  clock: [c(12, 12, 9), pl('M12 7v5l3 2')],
  alert: [p('m10.3 3.9-8.2 14A2 2 0 0 0 3.8 21h16.4a2 2 0 0 0 1.7-3.1l-8.2-14a2 2 0 0 0-3.4 0Z'), l(12, 9, 12, 13), l(12, 17, 12.01, 17)],
  info: [c(12, 12, 9), l(12, 11, 12, 16), l(12, 8, 12.01, 8)],
  user: [c(12, 8, 4), p('M4 21a8 8 0 0 1 16 0')],
  panelRight: [r(2, 3, 20, 18, 2), l(15, 3, 15, 21)],
  panelLeft: [r(2, 3, 20, 18, 2), l(9, 3, 9, 21)],
  more: [c(5, 12, 1), c(12, 12, 1), c(19, 12, 1)],
  edit: [p('M12 20h9'), p('M16.5 3.5a2.1 2.1 0 0 1 3 3L8 18l-4 1 1-4Z')],
  code: [pl('m16 18 6-6-6-6'), pl('m8 6-6 6 6 6')],
  boxes: [p('m12 2 9 5-9 5-9-5 9-5Z'), p('m3 12 9 5 9-5'), p('m3 17 9 5 9-5')],
  activity: [pl('M3 12h4l3-8 4 16 3-8h4')],
  shield: [p('M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10Z')],
  key: [c(7.5, 15.5, 4.5), p('m10.5 12.5 9-9 2 2-2 2 1.5 1.5-2.5 2.5-1.5-1.5-2 2')],
  globe: [c(12, 12, 9), p('M3 12h18'), p('M12 3a15 15 0 0 1 0 18'), p('M12 3a15 15 0 0 0 0 18')],
  zap: [p('M13 2 4 14h7l-1 8 9-12h-7l1-8Z')],
  link: [p('M10 13a5 5 0 0 0 7.5.5l3-3a5 5 0 0 0-7-7l-1.7 1.7'), p('M14 11a5 5 0 0 0-7.5-.5l-3 3a5 5 0 0 0 7 7l1.7-1.7')],
  external: [p('M15 3h6v6'), l(10, 14, 21, 3), p('M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6')],
  list: [l(8, 6, 21, 6), l(8, 12, 21, 12), l(8, 18, 21, 18), l(3, 6, 3.01, 6), l(3, 12, 3.01, 12), l(3, 18, 3.01, 18)],
  cpu: [r(4, 4, 16, 16, 2), r(9, 9, 6, 6, 1), l(9, 1, 9, 4), l(15, 1, 15, 4), l(9, 20, 9, 23), l(15, 20, 15, 23), l(1, 9, 4, 9), l(1, 15, 4, 15), l(20, 9, 23, 9), l(20, 15, 23, 15)],
  loader: [p('M12 2v4'), p('M12 18v4'), p('m4.9 4.9 2.8 2.8'), p('m16.3 16.3 2.8 2.8'), l(2, 12, 6, 12), l(18, 12, 22, 12), p('m4.9 19.1 2.8-2.8'), p('m16.3 7.7 2.8-2.8')],
  circle: [c(12, 12, 9)],
  keyboard: [r(2, 5, 20, 14, 2), l(6, 9, 6.01, 9), l(10, 9, 10.01, 9), l(14, 9, 14.01, 9), l(18, 9, 18.01, 9), l(6, 13, 6.01, 13), l(18, 13, 18.01, 13), l(8, 16, 16, 16)],
  play: [p('m7 4 13 8-13 8V4Z')],
  pause: [r(6, 4, 4, 16, 1), r(14, 4, 4, 16, 1)],
  image: [r(3, 3, 18, 18, 2), c(8.5, 8.5, 1.5), pl('m21 15-5-5L5 21')],
  eye: [p('M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7S2 12 2 12Z'), c(12, 12, 3)],
  lock: [r(4, 10, 16, 11, 2), p('M8 10V7a4 4 0 0 1 8 0v3')],
  share: [c(18, 5, 3), c(6, 12, 3), c(18, 19, 3), l(8.6, 10.5, 15.4, 6.5), l(8.6, 13.5, 15.4, 17.5)],
  history: [p('M3 12a9 9 0 1 0 3-6.7L3 8'), pl('M3 3v5h5'), pl('M12 7v5l4 2')],
  book: [p('M4 19.5A2.5 2.5 0 0 1 6.5 17H20'), p('M6.5 2H20v20H6.5A2.5 2.5 0 0 1 4 19.5v-15A2.5 2.5 0 0 1 6.5 2Z')],
  diff: [l(12, 4, 12, 20), l(4, 12, 20, 12), l(4, 6, 8, 6), l(16, 18, 20, 18)],
  cornerDownLeft: [pl('m9 10-5 5 5 5'), p('M20 4v7a4 4 0 0 1-4 4H4')],
  wrench: [p('M14.7 6.3a4 4 0 0 0-5.4 5.4L3 18l3 3 6.3-6.3a4 4 0 0 0 5.4-5.4L14 13l-3-3 3.7-3.7Z')],
};

export type IconName = keyof typeof shapes;

/** Build a Svelte component that always renders one fixed icon. */
function makeIcon(name: IconName) {
  const shape = shapes[name];
  const Component = class extends IconBase {
    constructor(options: any = {}) {
      super({ ...options, props: { ...(options.props || {}), shape, name } } as any);
    }
  };
  return Component as unknown as typeof IconBase;
}

export const Plus = makeIcon('plus');
export const Paperclip = makeIcon('paperclip');
export const Send = makeIcon('send');
export const ArrowUp = makeIcon('arrowUp');
export const ArrowDown = makeIcon('arrowDown');
export const ArrowRight = makeIcon('arrowRight');
export const ArrowLeft = makeIcon('arrowLeft');
export const Folder = makeIcon('folder');
export const FolderOpen = makeIcon('folderOpen');
export const File = makeIcon('file');
export const FileText = makeIcon('fileText');
export const FileDiff = makeIcon('fileDiff');
export const Terminal = makeIcon('terminal');
export const GitBranch = makeIcon('gitBranch');
export const GitCommit = makeIcon('gitCommit');
export const Plug = makeIcon('plug');
export const Sparkles = makeIcon('sparkles');
export const Server = makeIcon('server');
export const Settings = makeIcon('settings');
export const Check = makeIcon('check');
export const CheckCircle = makeIcon('checkCircle');
export const X = makeIcon('x');
export const ChevronDown = makeIcon('chevronDown');
export const ChevronRight = makeIcon('chevronRight');
export const ChevronLeft = makeIcon('chevronLeft');
export const ChevronUp = makeIcon('chevronUp');
export const Copy = makeIcon('copy');
export const Undo = makeIcon('undo');
export const Redo = makeIcon('redo');
export const Search = makeIcon('search');
export const Menu = makeIcon('menu');
export const Battery = makeIcon('battery');
export const Upload = makeIcon('upload');
export const Download = makeIcon('download');
export const Trash = makeIcon('trash');
export const Stop = makeIcon('stop');
export const Refresh = makeIcon('refresh');
export const Sun = makeIcon('sun');
export const Moon = makeIcon('moon');
export const Message = makeIcon('message');
export const Clock = makeIcon('clock');
export const Alert = makeIcon('alert');
export const Info = makeIcon('info');
export const User = makeIcon('user');
export const PanelRight = makeIcon('panelRight');
export const PanelLeft = makeIcon('panelLeft');
export const More = makeIcon('more');
export const Edit = makeIcon('edit');
export const Code = makeIcon('code');
export const Boxes = makeIcon('boxes');
export const Activity = makeIcon('activity');
export const Shield = makeIcon('shield');
export const Key = makeIcon('key');
export const Globe = makeIcon('globe');
export const Zap = makeIcon('zap');
export const Link = makeIcon('link');
export const External = makeIcon('external');
export const List = makeIcon('list');
export const Cpu = makeIcon('cpu');
export const Loader = makeIcon('loader');
export const Circle = makeIcon('circle');
export const Keyboard = makeIcon('keyboard');
export const Play = makeIcon('play');
export const Pause = makeIcon('pause');
export const Image = makeIcon('image');
export const Eye = makeIcon('eye');
export const Lock = makeIcon('lock');
export const Share = makeIcon('share');
export const History = makeIcon('history');
export const Book = makeIcon('book');
export const Diff = makeIcon('diff');
export const CornerDownLeft = makeIcon('cornerDownLeft');
export const Wrench = makeIcon('wrench');
