'use client';

import { createContext, useContext, useEffect, useMemo, type ReactNode } from 'react';
import type { Announcement } from '@/lib/types';

export type LocalAnnouncementDraft = {
  announcement: Announcement | null;
  title: string;
  body: string;
  conflict: boolean;
};

type DraftScope = {
  read: () => LocalAnnouncementDraft | null;
  retain: (draft: LocalAnnouncementDraft) => void;
  discard: () => void;
};
const DraftContext = createContext<DraftScope | null>(null);

// This is an in-memory navigation safety net, with no browser-storage or API
// writes. Changing accounts or losing either panel permission replaces the scope
// even when the announcement editor is absent from the current route.
export function AnnouncementDraftProvider({
  userId,
  authorized,
  children,
}: {
  userId: number | null;
  authorized: boolean;
  children: ReactNode;
}) {
  const scope = useMemo<DraftScope | null>(() => {
    if (!authorized || userId === null) return null;
    let draft: LocalAnnouncementDraft | null = null;
    return {
      read: () => draft,
      retain: (value) => {
        draft = value;
      },
      discard: () => {
        draft = null;
      },
    };
  }, [userId, authorized]);
  useEffect(
    () => () => {
      scope?.discard();
    },
    [scope],
  );
  return <DraftContext.Provider value={scope}>{children}</DraftContext.Provider>;
}

export const useAnnouncementDraft = () => useContext(DraftContext);
