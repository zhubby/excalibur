import type { Config } from "tailwindcss";

const color = (name: string) => `rgb(var(--color-${name}) / <alpha-value>)`;

const config: Config = {
  content: ["./src/**/*.{ts,tsx}"],
  theme: {
    extend: {
      colors: {
        ink: color("ink"),
        paper: color("paper"),
        panel: color("panel"),
        elevated: color("elevated"),
        rail: color("rail"),
        line: color("line"),
        muted: color("muted"),
        faint: color("faint"),
        brand: color("brand"),
        "brand-hover": color("brand-hover"),
        success: color("success"),
        warning: color("warning"),
        danger: color("danger"),
        signal: color("signal"),
      },
      boxShadow: {
        rail: "var(--shadow-rail)",
        panel: "var(--shadow-panel)",
      },
    },
  },
  plugins: [],
};

export default config;
