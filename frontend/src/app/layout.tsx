import type { Metadata } from "next";
import "./globals.css";

export const metadata: Metadata = {
  title: "Excalibur Console",
  description: "IoT fleet operations console",
};

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  return (
    <html lang="en">
      <body>{children}</body>
    </html>
  );
}

