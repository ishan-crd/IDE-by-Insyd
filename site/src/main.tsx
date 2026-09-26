import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import Lenis from 'lenis';
import App from './App';
import './styles.css';

// Smooth, inertial scrolling (skipped when the viewer asks for less motion).
if (!window.matchMedia('(prefers-reduced-motion: reduce)').matches) {
  const lenis = new Lenis({ lerp: 0.1 });
  const raf = (t: number) => {
    lenis.raf(t);
    requestAnimationFrame(raf);
  };
  requestAnimationFrame(raf);
  // In-page links glide instead of jumping.
  document.addEventListener('click', (e) => {
    const a = (e.target as HTMLElement).closest('a[href^="#"]') as HTMLAnchorElement | null;
    if (!a) return;
    e.preventDefault();
    const id = a.getAttribute('href')!;
    lenis.scrollTo(id === '#' ? 0 : id, { offset: -70 });
  });
}

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
