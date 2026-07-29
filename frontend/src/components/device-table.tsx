"use client";

import {
  ColumnDef,
  flexRender,
  getCoreRowModel,
  useReactTable,
} from "@tanstack/react-table";
import { Circle, Download, MoreHorizontal, RadioTower, Terminal } from "lucide-react";
import { useMemo } from "react";
import type { DeviceRow, DeviceStatus } from "@/lib/data";

const statusClass: Record<DeviceStatus, string> = {
  online: "bg-success/15 text-success",
  offline: "bg-danger/15 text-danger",
  disabled: "bg-elevated text-faint",
  provisioned: "bg-warning/15 text-warning",
};

type DeviceTableProps = {
  data: DeviceRow[];
  selectedDeviceId?: string;
  busy?: boolean;
  onCreateDevice: () => void;
  onSelectDevice: (deviceId: string) => void;
  onDownloadDevAuth: (deviceId: string) => void;
  onIngestSample: (deviceId: string) => void;
};

export function DeviceTable({
  data,
  selectedDeviceId,
  busy = false,
  onCreateDevice,
  onSelectDevice,
  onDownloadDevAuth,
  onIngestSample,
}: DeviceTableProps) {
  const columns = useMemo<ColumnDef<DeviceRow>[]>(
    () => [
      {
        accessorKey: "name",
        header: "Device",
        cell: ({ row }) => (
          <div className="min-w-0">
            <p className="truncate font-medium text-ink">{row.original.name}</p>
            <p className="truncate text-xs text-faint">{row.original.id}</p>
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
        cell: ({ row }) => <span className="truncate">{row.original.stream}</span>,
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
        cell: ({ row }) => <span>{row.original.rssi === null ? "-" : `${row.original.rssi} dBm`}</span>,
      },
      {
        id: "actions",
        header: "",
        cell: ({ row }) => (
          <div className="flex justify-end gap-1">
            <button
              className="grid h-8 w-8 place-items-center rounded-md text-muted transition hover:bg-line hover:text-ink disabled:cursor-not-allowed disabled:text-faint"
              type="button"
              aria-label={`Download dev auth for ${row.original.name}`}
              disabled={busy}
              onClick={(event) => {
                event.stopPropagation();
                onDownloadDevAuth(row.original.id);
              }}
            >
              <Download className="h-4 w-4" aria-hidden="true" />
            </button>
            <button
              className="grid h-8 w-8 place-items-center rounded-md text-muted transition hover:bg-line hover:text-ink disabled:cursor-not-allowed disabled:text-faint"
              type="button"
              aria-label={`Ingest sample telemetry for ${row.original.name}`}
              disabled={busy}
              onClick={(event) => {
                event.stopPropagation();
                onIngestSample(row.original.id);
              }}
            >
              <RadioTower className="h-4 w-4" aria-hidden="true" />
            </button>
            <button
              className="grid h-8 w-8 place-items-center rounded-md bg-elevated text-faint"
              type="button"
              aria-label="Remote shell disabled"
              disabled
            >
              <Terminal className="h-4 w-4" aria-hidden="true" />
            </button>
            <button
              className="grid h-8 w-8 place-items-center rounded-md text-muted transition hover:bg-line hover:text-ink"
              type="button"
              aria-label={`Select ${row.original.name}`}
              onClick={(event) => {
                event.stopPropagation();
                onSelectDevice(row.original.id);
              }}
            >
              <MoreHorizontal className="h-4 w-4" aria-hidden="true" />
            </button>
          </div>
        ),
      },
    ],
    [busy, onDownloadDevAuth, onIngestSample, onSelectDevice],
  );
  const table = useReactTable({
    data,
    columns,
    getCoreRowModel: getCoreRowModel(),
  });

  return (
    <section className="panel-in overflow-hidden rounded-md border border-line bg-panel shadow-panel">
      <div className="flex flex-col gap-2 border-b border-line px-4 py-3 sm:flex-row sm:items-center sm:justify-between">
        <div>
          <h2 className="text-base font-semibold text-ink">Devices</h2>
          <p className="text-sm text-muted">Provisioning, shadow state, firmware, and diagnostics.</p>
        </div>
        <button
          className="inline-flex h-9 items-center justify-center rounded-md bg-brand px-3 text-sm font-medium text-ink transition hover:bg-brand-hover disabled:cursor-not-allowed disabled:bg-elevated disabled:text-faint"
          type="button"
          disabled={busy}
          onClick={onCreateDevice}
        >
          Provision device
        </button>
      </div>
      <div className="overflow-x-auto">
        <table className="min-w-[920px] table-fixed border-collapse text-left text-sm">
          <thead className="bg-rail text-xs uppercase text-faint">
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
              <tr
                key={row.id}
                className={`cursor-pointer transition hover:bg-elevated ${
                  selectedDeviceId === row.original.id ? "bg-brand/10" : ""
                }`}
                onClick={() => onSelectDevice(row.original.id)}
              >
                {row.getVisibleCells().map((cell) => (
                  <td key={cell.id} className="px-4 py-3 text-muted">
                    {flexRender(cell.column.columnDef.cell, cell.getContext())}
                  </td>
                ))}
              </tr>
            ))}
            {table.getRowModel().rows.length === 0 ? (
              <tr>
                <td className="px-4 py-8 text-center text-sm text-muted" colSpan={columns.length}>
                  No devices in this project yet.
                </td>
              </tr>
            ) : null}
          </tbody>
        </table>
      </div>
    </section>
  );
}
