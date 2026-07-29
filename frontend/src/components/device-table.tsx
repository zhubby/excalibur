"use client";

import {
  ColumnDef,
  flexRender,
  getCoreRowModel,
  useReactTable,
} from "@tanstack/react-table";
import { Circle, MoreHorizontal, Terminal } from "lucide-react";
import type { DeviceRow, DeviceStatus } from "@/lib/data";

const statusClass: Record<DeviceStatus, string> = {
  online: "bg-teal/10 text-teal",
  offline: "bg-danger/10 text-danger",
  disabled: "bg-ink/10 text-ink/60",
  provisioned: "bg-amber/10 text-amber",
};

const columns: ColumnDef<DeviceRow>[] = [
  {
    accessorKey: "name",
    header: "Device",
    cell: ({ row }) => (
      <div className="min-w-0">
        <p className="truncate font-medium text-ink">{row.original.name}</p>
        <p className="truncate text-xs text-ink/48">{row.original.id}</p>
      </div>
    ),
  },
  {
    accessorKey: "status",
    header: "Status",
    cell: ({ row }) => (
      <span
        className={`inline-flex items-center gap-1.5 rounded-sm px-2 py-1 text-xs font-medium ${statusClass[row.original.status]}`}
      >
        <Circle className="h-2.5 w-2.5 fill-current" aria-hidden="true" />
        {row.original.status}
      </span>
    ),
  },
  {
    accessorKey: "stream",
    header: "Stream",
  },
  {
    accessorKey: "firmware",
    header: "Firmware",
  },
  {
    accessorKey: "lastSeen",
    header: "Last seen",
  },
  {
    accessorKey: "rssi",
    header: "RSSI",
    cell: ({ row }) => <span>{row.original.rssi === 0 ? "-" : `${row.original.rssi} dBm`}</span>,
  },
  {
    id: "actions",
    header: "",
    cell: () => (
      <div className="flex justify-end gap-1">
        <button
          className="grid h-8 w-8 place-items-center rounded-md text-ink/60 transition hover:bg-ink/5 hover:text-ink"
          type="button"
          aria-label="Open shell"
        >
          <Terminal className="h-4 w-4" aria-hidden="true" />
        </button>
        <button
          className="grid h-8 w-8 place-items-center rounded-md text-ink/60 transition hover:bg-ink/5 hover:text-ink"
          type="button"
          aria-label="More device actions"
        >
          <MoreHorizontal className="h-4 w-4" aria-hidden="true" />
        </button>
      </div>
    ),
  },
];

export function DeviceTable({ data }: { data: DeviceRow[] }) {
  const table = useReactTable({
    data,
    columns,
    getCoreRowModel: getCoreRowModel(),
  });

  return (
    <section className="panel-in overflow-hidden rounded-md border border-line bg-panel">
      <div className="flex flex-col gap-2 border-b border-line px-4 py-3 sm:flex-row sm:items-center sm:justify-between">
        <div>
          <h2 className="text-base font-semibold text-ink">Devices</h2>
          <p className="text-sm text-ink/54">Provisioning, shadow state, firmware, and diagnostics.</p>
        </div>
        <button className="inline-flex h-9 items-center justify-center rounded-md bg-teal px-3 text-sm font-medium text-white transition hover:bg-teal/90" type="button">
          Provision device
        </button>
      </div>
      <div className="overflow-x-auto">
        <table className="min-w-[920px] table-fixed border-collapse text-left text-sm">
          <thead className="bg-paper/60 text-xs uppercase text-ink/46">
            {table.getHeaderGroups().map((headerGroup) => (
              <tr key={headerGroup.id}>
                {headerGroup.headers.map((header) => (
                  <th key={header.id} className="px-4 py-3 font-semibold">
                    {header.isPlaceholder ? null : flexRender(header.column.columnDef.header, header.getContext())}
                  </th>
                ))}
              </tr>
            ))}
          </thead>
          <tbody className="divide-y divide-line">
            {table.getRowModel().rows.map((row) => (
              <tr key={row.id} className="transition hover:bg-paper/60">
                {row.getVisibleCells().map((cell) => (
                  <td key={cell.id} className="px-4 py-3 text-ink/72">
                    {flexRender(cell.column.columnDef.cell, cell.getContext())}
                  </td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </section>
  );
}
