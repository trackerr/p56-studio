import { createContext, useContext } from 'react';

export type TabId = 'fan' | 'playlist' | 'media' | 'cast' | 'network' | 'app';

const NavContext = createContext<(tab: TabId) => void>(() => {});

export const NavProvider = NavContext.Provider;
/** Switch to another tab (e.g. the "add" orbit in the playlist strip opens Media). */
export const useNav = () => useContext(NavContext);
