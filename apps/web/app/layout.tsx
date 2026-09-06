import type { Metadata, Viewport } from "next";
import type { ReactNode } from "react";
import "./globals.css";

export const metadata: Metadata = {
  title: "OH Hive",
  description: "The Hive — pooled compute for Office Hours Global and Loki's Lab. Share idle time, earn Honey, make things.",
  manifest: "/site.webmanifest",
  icons: {
    icon: [
      { url: "/favicon.svg", type: "image/svg+xml" },
      { url: "/favicon-32x32.png", sizes: "32x32", type: "image/png" },
      { url: "/favicon-16x16.png", sizes: "16x16", type: "image/png" },
    ],
    shortcut: "/favicon.ico",
    apple: "/apple-touch-icon.png",
  },
};

export const viewport: Viewport = {
  themeColor: "#1D1C20",
  colorScheme: "dark",
};

export default function RootLayout({ children }: { children: ReactNode }) {
  // Dark by default (Jack, 2026-09-05). Light is available via data-theme="light" on <html>.
  return (
    <html lang="en" data-theme="dark" suppressHydrationWarning>
      <head>
        {/* apply a remembered light/dark choice before first paint (no flash); dark is the default */}
        <script dangerouslySetInnerHTML={{ __html: `try{var t=localStorage.getItem("ohhive.theme");if(t==="light"||t==="dark")document.documentElement.setAttribute("data-theme",t)}catch(e){}` }} />
      </head>
      <body>{children}</body>
    </html>
  );
}
