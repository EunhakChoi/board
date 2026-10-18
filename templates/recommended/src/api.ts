export interface Task {
  id: number;
  title: string;
  done: boolean;
}

export async function fetchTasks(): Promise<Task[]> {
  const response = await fetch("/api/tasks");
  if (!response.ok) throw new Error(`GET /api/tasks failed: ${response.status}`);
  return (await response.json()) as Task[];
}
