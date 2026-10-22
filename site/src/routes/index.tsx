import { createFileRoute } from '@tanstack/react-router'
import { App } from '../app'
export const Route = createFileRoute('/')({
  head: () => ({ meta: [{ title: 'Milleleaf — Make content once. See what comes back.' }], links: [{ rel: 'canonical', href: 'https://milleleaf.com' }] }),
  component: () => <App path='/' />,
})
