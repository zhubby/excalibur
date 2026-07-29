import type { Config } from "tailwindcss";

const config: Config = {
  content: ["./src/**/*.{ts,tsx}"],
  theme: {
    extend: {
      colors: {
        ink: "#18211f",
        paper: "#f4f1ea",
        panel: "#fbfaf7",
        line: "#ded8cc",
        teal: "#16786f",
        amber: "#b87911",
        signal: "#2f5f9f",
        danger: "#b13d32",
      },
      boxShadow: {
        rail: "inset -1px 0 0 rgba(24,33,31,0.1)",
      },
    },
  },
  plugins: [],
};

export default config;

