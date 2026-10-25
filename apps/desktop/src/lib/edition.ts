import { createContext, useContext } from "react";

/**
 * Build flavour (ADR-0017). `direct` is the DMG "Spank" with every feature. `appstore` is the
 * Mac App Store edition "Creaky": sandboxed, lid sounds only (no slaps, calibration, precise typing
 * or updater). Rust reports the same value as `AppInfo.edition`.
 */
export type Edition = "direct" | "appstore";

/** Set at build time: the Mac App Store build runs `NEXT_PUBLIC_EDITION=appstore pnpm build`. */
export const BUILD_EDITION: Edition = process.env.NEXT_PUBLIC_EDITION === "appstore" ? "appstore" : "direct";

export const PRODUCT_NAMES: Record<Edition, string> = { direct: "Spank", appstore: "Creaky" };

/** Defaults to the build's edition; tests and previews can render the other one. */
export const EditionContext = createContext<Edition>(BUILD_EDITION);

export const useEdition = (): Edition => useContext(EditionContext);
export const useAppStore = (): boolean => useEdition() === "appstore";

/** The messages with this edition's product name as `app.name`. */
export function withProductName<M extends { app: { name: string } }>(messages: M, edition: Edition): M {
  return edition === "direct" ? messages : { ...messages, app: { ...messages.app, name: PRODUCT_NAMES[edition] } };
}
