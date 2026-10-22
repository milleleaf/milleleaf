import { createFileRoute } from '@tanstack/react-router'
import { App } from '../app'
export const Route = createFileRoute('/about')({
  head: () => ({ meta: [{ title: 'About | Milleleaf' }], links: [{ rel: 'canonical', href: 'https://milleleaf.com/about' }] }),
  component: () => <App path='/about' />,
})
