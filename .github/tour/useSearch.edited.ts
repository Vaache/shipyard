import { useEffect, useState } from "react"
import { fetchResults, type Result } from "./api"
import { useDebounce } from "../hooks/useDebounce"

export function useSearch(query: string) {
  const debounced = useDebounce(query, 250)
  const [results, setResults] = useState<Result[]>([])
  const [loading, setLoading] = useState(false)

  useEffect(() => {
    if (!debounced) return
    const controller = new AbortController()
    setLoading(true)
    fetchResults(debounced, controller.signal)
      .then(setResults)
      .catch((err) => {
        if (err.name !== "AbortError") throw err
      })
      .finally(() => {
        if (!controller.signal.aborted) setLoading(false)
      })
    return () => controller.abort()
  }, [debounced])

  return { results, loading }
}
