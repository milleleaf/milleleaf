import { createFileRoute } from '@tanstack/react-router'
import { App } from '../app'
export const Route = createFileRoute('/')({
  head: () => ({ meta: [{ title: 'Milleleaf | See the save. Keep the source.' }], links: [{ rel: 'canonical', href: 'https://milleleaf.com' }] }),
  component: () => <App path='/' />,
})
