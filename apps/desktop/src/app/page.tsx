"use client";

import { useRouter } from "next/navigation";
import { useEffect } from "react";

/** The settings window opens on Profiles. (Static export has no server-side redirects.) */
export default function Home() {
  const router = useRouter();
  useEffect(() => router.replace("/profiles/"), [router]);
  return null;
}
