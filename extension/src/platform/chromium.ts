// Chromium-only details. Other browsers get their own module here; nothing else changes.

interface Brand {
  brand: string;
}

/** Which Chromium browser this is, for the daemon's heartbeat list. */
export function browserName(): string {
  const nav = navigator as Navigator & {
    userAgentData?: { brands: Brand[] };
    brave?: unknown;
  };
  if (nav.brave) return "brave";
  const brands = nav.userAgentData?.brands.map(({ brand }) => brand) ?? [];
  if (brands.includes("Microsoft Edge")) return "edge";
  if (brands.some((brand) => brand.startsWith("Opera"))) return "opera"; // Opera, Opera GX
  if (brands.includes("Google Chrome")) return "chrome";
  return "chromium";
}
