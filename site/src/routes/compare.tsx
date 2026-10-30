import { createFileRoute } from '@tanstack/react-router'
import { App } from '../app'
export const Route = createFileRoute('/compare')({
  head: () => ({ meta: [{ title: 'Compared With Other Ways | Milleleaf' }], links: [{ rel: 'canonical', href: 'https://milleleaf.com/compare' }] }),
  component: () => <App path='/compare' />,
})
