import type { Metadata, Viewport } from "next";
import "./globals.css";

export const metadata: Metadata = {
  title: "Excalibur Console",
  description: "IoT fleet operations console",
};

export const viewport: Viewport = {
  width: "device-width",
  initialScale: 1,
};

const themeScript = `
(() => {
  try {
    const theme = window.localStorage.getItem("excalibur.console.theme.v1");
    document.documentElement.dataset.theme = theme === "light" ? "light" : "dark";
  } catch {
    document.documentElement.dataset.theme = "dark";
  }
})();
`;

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  return (
    <html lang="en" data-theme="dark" suppressHydrationWarning>
      <body>
        <script dangerouslySetInnerHTML={{ __html: themeScript }} />
        {children}
      </body>
    </html>
  );
}
