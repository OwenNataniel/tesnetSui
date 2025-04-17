import { SealClient, SessionKey, NoAccessError, EncryptedObject } from '@mysten/seal';
import { SuiClient } from '@mysten/sui/client';
import { Transaction } from '@mysten/sui/transactions';
import React, { useState } from 'react';

// Menentukan tipe untuk fungsi moveCallConstructor
export type MoveCallConstructor = (tx: Transaction, id: string) => void;

export const downloadAndDecrypt = async (
  blobIds: string[],
  sessionKey: SessionKey,
  suiClient: SuiClient,
  sealClient: SealClient,
  moveCallConstructor: MoveCallConstructor,
  setError: (error: string | null) => void,
  setPartialError: (error: string | null) => void,
  setDecryptedFileUrls: (urls: string[]) => void,
  setIsDialogOpen: (open: boolean) => void,
  setReloadKey: (updater: (prev: number) => number) => void,
) => {
  const aggregators = ['aggregator1', 'aggregator2', 'aggregator3', 'aggregator4', 'aggregator5', 'aggregator6'];

  try {
    // Unduh semua file secara paralel
    const downloadResults = await Promise.all(
      blobIds.map(async (blobId) => {
        try {
          const controller = new AbortController();
          const timeout = setTimeout(() => controller.abort(), 10000);
          const randomAggregator = aggregators[Math.floor(Math.random() * aggregators.length)];
          const aggregatorUrl = `/${randomAggregator}/v1/blobs/${blobId}`;
          const response = await fetch(aggregatorUrl, { signal: controller.signal });
          clearTimeout(timeout);
          if (!response.ok) {
            throw new Error(`Gagal mengambil blob ${blobId} dari Walrus`);
          }
          return await response.arrayBuffer();
        } catch (err) {
          console.error(`Gagal mengambil blob ${blobId}:`, err);
          return null;
        }
      })
    );

    // Filter file yang berhasil diunduh
    const validDownloads = downloadResults.filter((result): result is ArrayBuffer => result !== null);

    // Menampilkan kesalahan parsial jika tidak semua file bisa diunduh
    if (validDownloads.length < blobIds.length) {
      const errorMsg = `Menampilkan ${validDownloads.length} file dari ${blobIds.length} file. Yang lainnya tidak disimpan cukup lama di Walrus, harap unggah lagi.`;
      setPartialError(errorMsg);
    }

    if (validDownloads.length === 0) {
      const errorMsg = 'Tidak ada file yang berhasil diunduh dari aggregator Walrus. Harap coba lagi.';
      console.error(errorMsg);
      setError(errorMsg);
      return;
    }

    // Ambil kunci dalam batch <=10
    for (let i = 0; i < validDownloads.length; i += 10) {
      const batch = validDownloads.slice(i, i + 10);
      const ids = batch.map((enc) => EncryptedObject.parse(new Uint8Array(enc)).id);
      const tx = new Transaction();
      ids.forEach((id) => moveCallConstructor(tx, id));
      const txBytes = await tx.build({ client: suiClient, onlyTransactionKind: true });
      try {
        await sealClient.fetchKeys({ ids, txBytes, sessionKey, threshold: 2 });
      } catch (err) {
        console.log(err);
        const errorMsg =
          err instanceof NoAccessError
            ? 'Tidak ada akses ke kunci dekripsi'
            : 'Tidak dapat mendekripsi file, coba lagi';
        console.error(errorMsg, err);
        setError(errorMsg);
        return;
      }
    }

    // Dekripsi file secara berurutan
    const decryptedFileUrls: string[] = [];
    for (const encryptedData of validDownloads) {
      const fullId = EncryptedObject.parse(new Uint8Array(encryptedData)).id;
      const tx = new Transaction();
      moveCallConstructor(tx, fullId);
      const txBytes = await tx.build({ client: suiClient, onlyTransactionKind: true });
      try {
        const decryptedFile = await sealClient.decrypt({
          data: new Uint8Array(encryptedData),
          sessionKey,
          txBytes,
        });
        const blob = new Blob([decryptedFile], { type: 'image/jpg' });
        decryptedFileUrls.push(URL.createObjectURL(blob));
      } catch (err) {
        console.log(err);
        const errorMsg =
          err instanceof NoAccessError
            ? 'Tidak ada akses ke kunci dekripsi'
            : 'Tidak dapat mendekripsi file, coba lagi';
        console.error(errorMsg, err);
        setError(errorMsg);
        return;
      }
    }

    // Update UI dengan file yang berhasil didekripsi
    if (decryptedFileUrls.length > 0) {
      setDecryptedFileUrls(decryptedFileUrls);
      setIsDialogOpen(true);
      setReloadKey((prev) => prev + 1);
    }

  } catch (err) {
    const errorMsg = 'Terjadi kesalahan saat mendownload atau mendekripsi file.';
    console.error(errorMsg, err);
    setError(errorMsg);
  }
};

// Komponen untuk menampilkan tautan ke penjelajah objek
export const getObjectExplorerLink = (id: string): React.ReactElement => {
  return React.createElement(
    'a',
    {
      href: `https://testnet.suivision.xyz/object/${id}`,
      target: '_blank',
      rel: 'noopener noreferrer',
      style: { textDecoration: 'underline' },
    },
    id.slice(0, 10) + '...',
  );
};
