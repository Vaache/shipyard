import { useEffect, useState } from "react"
import { fetchResults, type Result } from "./api"
import { useDebounce } from "../hooks/useDebounce"

export function useSearch(query: string) {
  const debounced = useDebounce(query, 250)
  const [results, setResults] = useState<Result[]>([])
  const [loading, setLoading] = useState(false)

  useEffect(() => {
    if (!debounced) return
    setLoading(true)
    fetchResults(debounced)
      .then(setResults)
      .finally(() => setLoading(false))
  }, [debounced])

  return { results, loading }
}
