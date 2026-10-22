import { createFileRoute, Outlet } from '@tanstack/react-router'
export const Route = createFileRoute('/docs')({
  head: () => ({ meta: [{ title: 'Docs | Milleleaf' }], links: [{ rel: 'canonical', href: 'https://milleleaf.com/docs' }] }),
  component: Outlet,
})
