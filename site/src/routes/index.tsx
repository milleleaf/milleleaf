import { createFileRoute } from '@tanstack/react-router'
import { App } from '../app'
export const Route = createFileRoute('/')({
  head: () => ({ meta: [{ title: 'Milleleaf | Make it once. Use it in every app.' }], links: [{ rel: 'canonical', href: 'https://milleleaf.com' }] }),
  component: () => <App path='/' />,
})
