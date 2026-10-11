// Line icons (24×24, stroke = currentColor).
import type { ReactNode, SVGProps } from "react";

type P = SVGProps<SVGSVGElement> & { size?: number };

function Svg({ size = 18, children, ...rest }: P & { children: ReactNode }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.8}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      {...rest}
    >
      {children}
    </svg>
  );
}

export const IHome = (p: P) => (
  <Svg {...p}>
    <path d="M4 10.5 12 4l8 6.5V19a1 1 0 0 1-1 1h-4.5v-5.5h-5V20H5a1 1 0 0 1-1-1z" />
  </Svg>
);
export const ILyrics = (p: P) => (
  <Svg {...p}>
    <path d="M4 6.5h9M4 11h6M4 15.5h5" />
    <path d="M17.5 16V6.2l3 1.3" />
    <circle cx="15.5" cy="16" r="2" />
  </Svg>
);
export const IChat = (p: P) => (
  <Svg {...p}>
    <path d="M5 5.5h14a1.5 1.5 0 0 1 1.5 1.5v8.5A1.5 1.5 0 0 1 19 17h-7l-4.5 3.5V17H5a1.5 1.5 0 0 1-1.5-1.5V7A1.5 1.5 0 0 1 5 5.5z" />
    <path d="M9.5 9.5l.6 1.4 1.4.6-1.4.6-.6 1.4-.6-1.4-1.4-.6 1.4-.6zM15 9h1.5M14 12.5h2.5" />
  </Svg>
);
export const IPaperclip = (p: P) => (
  <Svg {...p}>
    <path d="m19.5 11.5-7.4 7.4a4.6 4.6 0 0 1-6.5-6.5l7.8-7.8a3 3 0 0 1 4.3 4.3l-7.6 7.6a1.5 1.5 0 0 1-2.1-2.1l7-7" />
  </Svg>
);
export const ISend = (p: P) => (
  <Svg {...p}>
    <path d="M12 19.5V5M6 11l6-6 6 6" />
  </Svg>
);
export const IStop = (p: P) => (
  <Svg {...p}>
    <rect x="6.5" y="6.5" width="11" height="11" rx="2.5" fill="currentColor" stroke="none" />
  </Svg>
);
export const ISun = (p: P) => (
  <Svg {...p}>
    <circle cx="12" cy="12" r="3.8" />
    <path d="M12 3.5v1.8M12 18.7v1.8M3.5 12h1.8M18.7 12h1.8M6 6l1.3 1.3M16.7 16.7 18 18M6 18l1.3-1.3M16.7 7.3 18 6" />
  </Svg>
);
export const IMic = (p: P) => (
  <Svg {...p}>
    <rect x="9" y="3.5" width="6" height="11" rx="3" />
    <path d="M5.5 11.5a6.5 6.5 0 0 0 13 0M12 18v2.5" />
  </Svg>
);
export const IDownload = (p: P) => (
  <Svg {...p}>
    <path d="M12 4.5v10M7.5 10.5 12 15l4.5-4.5M5 19.5h14" />
  </Svg>
);
export const IChevron = (p: P) => (
  <Svg {...p}>
    <path d="m7 10 5 5 5-5" />
  </Svg>
);
export const INote = (p: P) => (
  <Svg {...p}>
    <path d="M6 3.5h9l3.5 3.5v12a1.5 1.5 0 0 1-1.5 1.5H6A1.5 1.5 0 0 1 4.5 19V5A1.5 1.5 0 0 1 6 3.5z" />
    <path d="M14.5 3.5V7.5h4M8 11.5h8M8 15h5.5" />
  </Svg>
);
export const ITextTool = (p: P) => (
  <Svg {...p}>
    <path d="M4 7V5h10v2M9 5v13M7 18h4" />
    <path d="M14.5 13.5h6M17.5 11v8" />
  </Svg>
);
export const IVideo = (p: P) => (
  <Svg {...p}>
    <rect x="3.5" y="6.5" width="12" height="11" rx="2.5" />
    <path d="m15.5 10.5 5-3v9l-5-3" />
  </Svg>
);
export const IHeart = ({ filled, ...p }: P & { filled?: boolean }) => (
  <Svg {...p}>
    <path
      d="M12 19.5s-7.5-4.3-7.5-9.6A4.1 4.1 0 0 1 12 7.3a4.1 4.1 0 0 1 7.5 2.6c0 5.3-7.5 9.6-7.5 9.6z"
      fill={filled ? "currentColor" : "none"}
    />
  </Svg>
);
export const IHeartOff = ({ filled, ...p }: P & { filled?: boolean }) => (
  <Svg {...p}>
    <path
      d="M12 19.5s-7.5-4.3-7.5-9.6A4.1 4.1 0 0 1 12 7.3a4.1 4.1 0 0 1 7.5 2.6c0 5.3-7.5 9.6-7.5 9.6z"
      fill={filled ? "currentColor" : "none"}
      opacity={filled ? 0.9 : 1}
    />
    <path d="M4 4.5 20 20.5" strokeWidth={2.2} stroke={filled ? "var(--card, #121214)" : "currentColor"} />
    <path d="M4 4.5 20 20.5" />
  </Svg>
);
export const IClipboard = (p: P) => (
  <Svg {...p}>
    <rect x="5" y="4.5" width="14" height="16" rx="2.5" />
    <path d="M9 4.5V4a1 1 0 0 1 1-1h4a1 1 0 0 1 1 1v.5M9 10h6M9 13.5h6M9 17h3.5" />
  </Svg>
);
export const IInbox = (p: P) => (
  <Svg {...p}>
    <path d="M4 13.5 6.2 6.2A1.8 1.8 0 0 1 7.9 5h8.2a1.8 1.8 0 0 1 1.7 1.2L20 13.5V18a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2z" />
    <path d="M4 13.5h4.5l1.2 2.2h4.6l1.2-2.2H20" />
  </Svg>
);
export const ISliders = (p: P) => (
  <Svg {...p}>
    <path d="M4 7h9M17 7h3M4 17h3M11 17h9" />
    <circle cx="15" cy="7" r="2" />
    <circle cx="9" cy="17" r="2" />
  </Svg>
);
export const IPin = (p: P) => (
  <Svg {...p}>
    <path d="M9 3.5h6M10 3.5v5.2L7 13h10l-3-4.3V3.5M12 13v7.5" />
  </Svg>
);
export const ISearch = (p: P) => (
  <Svg {...p}>
    <circle cx="11" cy="11" r="6.5" />
    <path d="m16 16 4 4" />
  </Svg>
);
export const ITrash = (p: P) => (
  <Svg {...p}>
    <path d="M4.5 7h15M9.5 7V5a1 1 0 0 1 1-1h3a1 1 0 0 1 1 1v2M6.5 7l.8 12a1.5 1.5 0 0 0 1.5 1.4h6.4a1.5 1.5 0 0 0 1.5-1.4l.8-12M10 11v6M14 11v6" />
  </Svg>
);
export const IPlay = (p: P) => (
  <Svg {...p} fill="currentColor" stroke="none">
    <path d="M8 5.6v12.8a1 1 0 0 0 1.5.86l10.4-6.4a1 1 0 0 0 0-1.72L9.5 4.74A1 1 0 0 0 8 5.6z" />
  </Svg>
);
export const IPause = (p: P) => (
  <Svg {...p} fill="currentColor" stroke="none">
    <rect x="6.5" y="5" width="4" height="14" rx="1.2" />
    <rect x="13.5" y="5" width="4" height="14" rx="1.2" />
  </Svg>
);
export const IPrev = (p: P) => (
  <Svg {...p} fill="currentColor" stroke="none">
    <rect x="5" y="5.5" width="2.4" height="13" rx="1" />
    <path d="M19 6.4v11.2a1 1 0 0 1-1.55.83L9.3 13a1.2 1.2 0 0 1 0-2l8.15-5.43A1 1 0 0 1 19 6.4z" />
  </Svg>
);
export const INext = (p: P) => (
  <Svg {...p} fill="currentColor" stroke="none">
    <rect x="16.6" y="5.5" width="2.4" height="13" rx="1" />
    <path d="M5 6.4v11.2a1 1 0 0 0 1.55.83L14.7 13a1.2 1.2 0 0 0 0-2L6.55 5.57A1 1 0 0 0 5 6.4z" />
  </Svg>
);
export const IVolume = (p: P) => (
  <Svg {...p}>
    <path d="M4 9.5h3.5L12 5.5v13l-4.5-4H4z" />
    <path d="M15.5 9a4 4 0 0 1 0 6M18 6.5a7.5 7.5 0 0 1 0 11" />
  </Svg>
);
export const IMute = (p: P) => (
  <Svg {...p}>
    <path d="M4 9.5h3.5L12 5.5v13l-4.5-4H4z" />
    <path d="m16 9.5 5 5M21 9.5l-5 5" />
  </Svg>
);
export const ISpeaker = (p: P) => (
  <Svg {...p}>
    <rect x="6" y="3.5" width="12" height="17" rx="2.5" />
    <circle cx="12" cy="14.5" r="3" />
    <circle cx="12" cy="7.5" r="0.8" fill="currentColor" />
  </Svg>
);
export const ICalc = (p: P) => (
  <Svg {...p}>
    <rect x="5" y="3.5" width="14" height="17" rx="2.5" />
    <path d="M8.5 7.5h7M8.5 12h.01M12 12h.01M15.5 12h.01M8.5 15.5h.01M12 15.5h.01M15.5 15.5h.01" />
  </Svg>
);
export const IGlobe = (p: P) => (
  <Svg {...p}>
    <circle cx="12" cy="12" r="8.5" />
    <path d="M3.5 12h17M12 3.5c2.4 2.5 3.5 5.3 3.5 8.5s-1.1 6-3.5 8.5c-2.4-2.5-3.5-5.3-3.5-8.5s1.1-6 3.5-8.5z" />
  </Svg>
);
export const ISwap = (p: P) => (
  <Svg {...p}>
    <path d="M5 8.5h13l-3.5-3.5M19 15.5H6l3.5 3.5" />
  </Svg>
);
export const IEnter = (p: P) => (
  <Svg {...p}>
    <path d="M19 5v6.5a2 2 0 0 1-2 2H6M9.5 10 6 13.5 9.5 17" />
  </Svg>
);
export const IFolder = (p: P) => (
  <Svg {...p}>
    <path d="M3.5 7.5a2 2 0 0 1 2-2h4l2 2h7a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2h-13a2 2 0 0 1-2-2z" />
  </Svg>
);
export const IFile = (p: P) => (
  <Svg {...p}>
    <path d="M13.5 3.5H7a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V9z" />
    <path d="M13.5 3.5V9H19" />
  </Svg>
);
export const IImage = (p: P) => (
  <Svg {...p}>
    <rect x="3.5" y="5" width="17" height="14" rx="2.5" />
    <circle cx="9" cy="10" r="1.6" />
    <path d="m4 17 4.5-4.5 3.5 3.5 2.5-2.5L20 18.5" />
  </Svg>
);
export const IText = (p: P) => (
  <Svg {...p}>
    <path d="M6 6.5V5h12v1.5M12 5v14M9.5 19h5" />
  </Svg>
);
export const IClose = (p: P) => (
  <Svg {...p}>
    <path d="m6.5 6.5 11 11M17.5 6.5l-11 11" />
  </Svg>
);
export const IPlus = (p: P) => (
  <Svg {...p}>
    <path d="M12 5v14M5 12h14" />
  </Svg>
);
export const ICopy = (p: P) => (
  <Svg {...p}>
    <rect x="8.5" y="8.5" width="11" height="11" rx="2.2" />
    <path d="M15.5 8.5V6.5a2 2 0 0 0-2-2h-7a2 2 0 0 0-2 2v7a2 2 0 0 0 2 2h2" />
  </Svg>
);
export const IRegion = (p: P) => (
  <Svg {...p}>
    <path d="M4 8V5.5A1.5 1.5 0 0 1 5.5 4H8M12 4h2M18 4h.5A1.5 1.5 0 0 1 20 5.5V8M4 12v2M4 18v.5A1.5 1.5 0 0 0 5.5 20H8" />
    <path d="m12.5 12.5 7.5 2.8-3.2 1.4-1.4 3.2z" />
  </Svg>
);
export const IFullscreen = (p: P) => (
  <Svg {...p}>
    <rect x="3.5" y="5" width="17" height="12" rx="2" />
    <path d="M9 20h6M12 17v3" />
    <path d="M7 9.5V8h1.5M17 9.5V8h-1.5M7 12.5V14h1.5M17 12.5V14h-1.5" />
  </Svg>
);
export const IScanText = (p: P) => (
  <Svg {...p}>
    <path d="M4 8V5.5A1.5 1.5 0 0 1 5.5 4H8M16 4h2.5A1.5 1.5 0 0 1 20 5.5V8M20 16v2.5a1.5 1.5 0 0 1-1.5 1.5H16M8 20H5.5A1.5 1.5 0 0 1 4 18.5V16" />
    <path d="M8 9h8M12 9v7" />
  </Svg>
);
export const IQr = (p: P) => (
  <Svg {...p}>
    <rect x="4" y="4" width="6" height="6" rx="1" />
    <rect x="14" y="4" width="6" height="6" rx="1" />
    <rect x="4" y="14" width="6" height="6" rx="1" />
    <path d="M14 14h2.5v2.5H14zM18.5 14H20M20 18.5V20h-3.5M14 19.5V20" />
  </Svg>
);
export const IPipette = (p: P) => (
  <Svg {...p}>
    <path d="m14.5 6.5 3 3" />
    <path d="M16 5l1.6-1.6a2 2 0 0 1 2.8 2.8L19 7.8M16.5 4.5l3 3L9 18l-4 1 1-4z" />
  </Svg>
);
export const IRecord = (p: P) => (
  <Svg {...p}>
    <circle cx="12" cy="12" r="8" />
    <circle cx="12" cy="12" r="3.2" fill="currentColor" stroke="none" />
  </Svg>
);
export const IEraser = (p: P) => (
  <Svg {...p}>
    <path d="m14.5 4.5 5 5a1.5 1.5 0 0 1 0 2.1L12 19H7.5l-3-3a1.5 1.5 0 0 1 0-2.1l7.9-9.4a1.5 1.5 0 0 1 2.1 0z" />
    <path d="M9 10.5 14.5 16M12 19h8" />
  </Svg>
);
export const ICommand = (p: P) => (
  <Svg {...p}>
    <rect x="3.5" y="4.5" width="17" height="15" rx="2.5" />
    <path d="m7.5 9.5 3 2.5-3 2.5M12.5 15h4" />
  </Svg>
);
export const ILock = (p: P) => (
  <Svg {...p}>
    <rect x="5" y="10.5" width="14" height="10" rx="2.5" />
    <path d="M8 10.5V8a4 4 0 0 1 8 0v2.5M12 14.5v2" />
  </Svg>
);
export const IReveal = (p: P) => (
  <Svg {...p}>
    <path d="M3.5 7.5a2 2 0 0 1 2-2h4l2 2h7a2 2 0 0 1 2 2v1.5" />
    <path d="M3.5 7.5v9a2 2 0 0 0 2 2H11M14 18.5h6.5M17.5 15.5l3 3-3 3" />
  </Svg>
);
export const IPower = (p: P) => (
  <Svg {...p}>
    <path d="M12 3.5v8M7.2 6.5a7.5 7.5 0 1 0 9.6 0" />
  </Svg>
);
export const ILauncher = (p: P) => (
  <Svg {...p}>
    <rect x="4" y="4" width="6.5" height="6.5" rx="1.6" />
    <rect x="13.5" y="4" width="6.5" height="6.5" rx="1.6" />
    <rect x="4" y="13.5" width="6.5" height="6.5" rx="1.6" />
    <path d="M16.75 13.5v6.5M13.5 16.75H20" />
  </Svg>
);
export const ICheck = (p: P) => (
  <Svg {...p}>
    <path d="m5 12.5 4.5 4.5L19 7.5" />
  </Svg>
);
export const IMusic = (p: P) => (
  <Svg {...p}>
    <path d="M9 18V6l10-2v12" />
    <circle cx="6.5" cy="18" r="2.5" />
    <circle cx="16.5" cy="16" r="2.5" />
  </Svg>
);
export const IKeyboard = (p: P) => (
  <Svg {...p}>
    <rect x="3" y="6" width="18" height="12" rx="2.5" />
    <path d="M7 10h.01M10 10h.01M13 10h.01M16 10h.01M8 14h8" />
  </Svg>
);
export const IAlert = (p: P) => (
  <Svg {...p}>
    <circle cx="12" cy="12" r="8.5" />
    <path d="M12 7.5v5.5M12 16.5h.01" />
  </Svg>
);
